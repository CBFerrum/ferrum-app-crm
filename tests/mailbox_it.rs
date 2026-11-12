//! CRM beside the real Mail (`ferrum-app-comms`) against a real PostgreSQL (`FERRUM_CRM_TEST_DATABASE_URL`):
//! the connections the package offers (CR-2), pulled through the platform's test connections (each operation answers
//! from its fixture, nobody is called), and what the sequences send through Mail (item 4, below).

use ferrum_kernel::connections::ConnectionInput;
use ferrum_kernel::tenant::NewTenant;
use ferrum_kernel::{Ctx, Db, DbConfig, Doc, HookRegistry, Kernel, KernelSettings, ListQuery};
use ferrum_meta::Catalog;
use serde_json::{Value, json};
use std::sync::Arc;
use uuid::Uuid;

const PASSWORD: &str = "Correct-Horse-9";

fn obj(v: Value) -> Doc {
    v.as_object().cloned().unwrap()
}
fn uid(doc: &Doc) -> Uuid {
    doc["id"].as_str().unwrap().parse().unwrap()
}

struct U {
    k: Arc<Kernel>,
    ctx: Ctx,
}

impl U {
    async fn insert(&self, entity: &str, v: Value) -> Doc {
        self.k.insert(&self.ctx, entity, obj(v)).await.unwrap_or_else(|e| panic!("insert {entity}: {e:?}"))
    }
    async fn get(&self, entity: &str, id: Uuid) -> Doc {
        self.k.get(&self.ctx, entity, id, false).await.unwrap_or_else(|e| panic!("get {entity}: {e:?}"))
    }
    #[allow(dead_code)]
    async fn action(&self, entity: &str, id: Uuid, action: &str, params: Value) -> Value {
        self.k.run_action(&self.ctx, entity, id, action, params).await.unwrap_or_else(|e| panic!("{entity}.{action}: {e:?}"))
    }
    async fn list(&self, entity: &str, filters: Value) -> Vec<Doc> {
        let q = ListQuery { filters: ferrum_kernel::query::parse_filters(&filters).unwrap(), limit: Some(500), ..Default::default() };
        self.k.list(&self.ctx, entity, &q).await.unwrap_or_else(|e| panic!("list {entity}: {e:?}")).rows
    }
    async fn timeline(&self, entity: &str, id: Uuid) -> Value {
        ferrum_app_crm::timeline(&self.k, &self.ctx, entity, id, &Default::default()).await.unwrap_or_else(|e| panic!("timeline {entity}: {e:?}"))
    }
}

async fn fresh_kernel() -> Arc<Kernel> {
    let url = std::env::var("FERRUM_CRM_TEST_DATABASE_URL").unwrap_or_else(|_| "postgres://ferrum:ferrum@localhost:5432/ferrum_crm_test".into());
    let db = Db::connect(DbConfig { url, max_connections: 10, ..Default::default() }).await.expect("connect");
    sqlx::raw_sql("drop schema if exists public cascade; create schema public;").execute(&db.pool).await.expect("reset");
    let mut packages = ferrum_domain::packages().expect("platform packages");
    packages.extend(ferrum_app_erp::packages().expect("ERP's packages"));
    packages.extend(ferrum_app_crm::packages().expect("CRM's packages"));
    packages.extend(ferrum_app_comms::packages().expect("Mail's packages"));
    let mut hooks = HookRegistry::default();
    ferrum_domain::register(&mut hooks);
    ferrum_app_erp::register(&mut hooks);
    ferrum_app_crm::register(&mut hooks);
    ferrum_app_comms::register(&mut hooks);
    for source in ferrum_app_erp::reports().into_iter().chain(ferrum_app_crm::reports()) {
        ferrum_domain::reports::add_source(source);
    }
    let settings = KernelSettings {
        secret_key: Some(ferrum_kernel::secret::SecretKey::new([7u8; 32])),
        public_url: Some("https://ferrum.test".into()),
        ..Default::default()
    };
    let k = Kernel::boot(db, Catalog::from_packages(packages).expect("catalog"), hooks, settings, None).await.expect("boot");
    ferrum_domain::maintenance(&k).await.expect("maintenance");
    ferrum_app_erp::maintenance(&k).await.expect("maintenance");
    k
}

async fn drain(k: &Kernel) {
    for _ in 0..4 {
        let mut tx = k.db.system_tx().await.unwrap();
        sqlx::query("update job set run_at = now() where status = 'queued'").execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
        while k.run_jobs_once(4).await.unwrap() > 0 {}
        while k.dispatch_outbox_once().await.expect("dispatch") > 0 {}
    }
}

fn titles(t: &Value, kind: &str) -> Vec<String> {
    t["events"].as_array().unwrap().iter().filter(|e| e["kind"] == kind).filter_map(|e| e["title"].as_str().map(str::to_string)).collect()
}

#[test]
fn crm_beside_the_real_mail() {
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().thread_stack_size(16 << 20).build().unwrap();
    rt.block_on(async { tokio::spawn(scenario()).await.unwrap() });
}

async fn scenario() {
    let k = fresh_kernel().await;
    let tenant = k
        .create_tenant(NewTenant {
            slug: "mailbox".into(),
            name: "Mailbox Ltd".into(),
            admin_email: "admin@mailbox.test".into(),
            admin_password: PASSWORD.into(),
            admin_name: None,
            packages: vec!["crm".into(), "crm_erp".into(), "comms".into()],
            settings: Some(json!({"timezone": "Etc/UTC"})),
        })
        .await
        .expect("tenant");
    let session = k.login("mailbox", "admin@mailbox.test", PASSWORD).await.expect("login");
    let admin = U { k: k.clone(), ctx: k.load_ctx(tenant, session.user, Some(session.token_version), "it".into()).await.expect("ctx") };
    let inr = admin.list("Currency", json!({"code": "INR"})).await.remove(0);
    let company = admin.insert("Company", json!({"company_name": "Mailbox Ltd", "abbr": "MB", "default_currency": inr["id"]})).await["id"].clone();
    let anna =
        k.create_user(&admin.ctx, "anna@mailbox.test", "Anna Rep", Some(PASSWORD), &["Sales User".to_string(), "Comms User".to_string()], false).await.unwrap();
    let _ = anna;
    connections(&admin, tenant, &company).await;
    sequence_mail(&admin, tenant, &company).await;
}

/// The two connections CRM offers: valid as shipped and off; pulled through a test connection, the headers and the
/// events land on Mail's records and CRM files them; a person-owned connection cannot be pulled.
async fn connections(admin: &U, tenant: Uuid, company: &Value) {
    let k = &admin.k;
    let offered = k.connection_defs(&admin.ctx).await.unwrap();
    let shipped = |name: &str| offered.iter().find(|c| c["name"] == name).unwrap_or_else(|| panic!("{name} is offered: {offered:?}")).clone();
    for name in ["crm_shared_mailbox", "crm_personal_mailbox"] {
        let c = shipped(name);
        assert_eq!(
            (c["enabled"].clone(), c["id"].clone(), c["offered"].clone()),
            (json!(false), Value::Null, json!(true)),
            "{name} arrives off, not yet taken up"
        );
        assert_eq!(c["source"], json!("package:crm"));
        // as shipped it is a description the platform accepts (placeholders and all), saved off
        let input: ConnectionInput =
            serde_json::from_value(json!({"name": name, "label": c["label"], "description": c["description"], "enabled": false})).unwrap();
        k.save_connection_def(&admin.ctx, None, &input).await.unwrap_or_else(|e| panic!("{name} as shipped: {e:?}"));
    }
    let shared = shipped("crm_shared_mailbox")["description"].clone();
    assert!(shared["pulls"].as_array().unwrap().len() == 1 && shipped("crm_personal_mailbox")["description"]["pulls"].is_null());
    let asked = format!("{}{}", shared["operations"], shared["pulls"]);
    assert!(!asked.contains("body") && !asked.contains("content") && !asked.contains("attachment"), "headers only: nothing of a body is asked for or mapped");

    // ── pulled: a test connection answers each operation from its fixture
    let jane =
        uid(&admin.insert("Contact", json!({"first_name": "Jane", "last_name": "Lee", "emails": [{"email": "jane@acme.test", "is_primary": true}]})).await);
    let pat = uid(&admin
        .insert("Lead", json!({"first_name": "Pat", "last_name": "Prospect", "email": "pat@lead.test", "source": "Website", "company": company}))
        .await);
    let now = chrono::Utc::now();
    let at = |h: i64| (now + chrono::Duration::hours(h)).to_rfc3339();
    let messages = |extra: Value| {
        let mut list = vec![
            json!({"id": "p1", "message_id": "mid-1@provider.test", "subject": "Spares quote", "from": {"address": "JANE@acme.test", "name": "Jane Lee"}, "to": [{"address": "sales@mailbox.test"}],
                   "cc": [{"address": "boss@acme.test"}], "received_at": at(-2), "body": "A SECRET BODY", "attachments": [{"name": "secret.pdf"}]}),
            json!({"id": "p2", "subject": "Hello from Pat", "from": {"address": "pat@lead.test"}, "to": [{"address": "sales@mailbox.test"}], "received_at": at(-1)}),
            json!({"id": "p3", "message_id": "mid-3@provider.test", "subject": "Nobody knows me", "from": {"address": "stranger@elsewhere.test"}, "received_at": at(-1)}),
        ];
        if let Some(x) = extra.as_object() {
            list.push(Value::Object(x.clone()));
        }
        json!({"status": 200, "body": {"messages": list, "next_page_token": null}})
    };
    let events = |start: i64, status: &str| {
        json!({"status": 200, "body": {"events": [{"id": "ev1", "title": "Spares call", "start": at(start), "end": at(start + 1), "status": status,
                "organizer": {"address": "anna@mailbox.test"}, "attendees": [{"address": "jane@acme.test"}, {"address": "anna@mailbox.test"}]}], "next_page_token": null}})
    };
    let as_test = |fixture_messages: Value, fixture_events: Value, with_events: bool| {
        let mut d = shared.clone();
        d["test"] = json!(true);
        d["operations"]["list_messages"]["fixture"] = fixture_messages;
        d["operations"]["list_events"]["fixture"] = fixture_events;
        if with_events {
            // not shipped (below): what mapping events onto Mail's CalendarEvent would be
            d["pulls"].as_array_mut().unwrap().push(
                json!({"name": "events", "operation": "list_events", "every_minutes": 15, "items_at": "/events", "cursor": "token",
                "next_at": "/next_page_token", "map": {"entity": "CalendarEvent", "by": {"ical_uid": "item.id", "owner": "item.organizer.address"},
                "set": {"subject": "item.title", "starts_at": "item.start", "ends_at": "item.end", "attendees": "item.attendees.map(a, {'email': a.address})"},
                "create": {"fetched": "true", "visibility": "'Private'"}}}),
            );
        }
        let d: ConnectionInput =
            serde_json::from_value(json!({"name": "crm_shared_mailbox", "label": "Shared mailbox", "description": d, "enabled": true})).unwrap();
        d
    };
    // taking up the offered connection: the same name, so the same connection as the shipped one saved off above
    let listed = k.connection_defs(&admin.ctx).await.unwrap();
    let id: Uuid = listed.iter().find(|c| c["name"] == "crm_shared_mailbox").unwrap()["id"].as_str().unwrap().parse().unwrap();
    k.save_connection_def(&admin.ctx, Some(id), &as_test(messages(Value::Null), events(24, "confirmed"), false)).await.expect("the test variant");
    let pull = |name: &'static str| async move {
        k.pull_now(&admin.ctx, id, name).await.unwrap();
        drain(k).await;
    };
    pull("messages").await;
    let pulls = k.connection_pulls(&admin.ctx, id).await.unwrap();
    assert!(pulls.iter().all(|p| p["last_error"].is_null()), "{pulls:?}");
    let comms = admin.list("Communication", json!({})).await;
    assert_eq!(comms.len(), 3, "{}", comms.iter().map(|c| c["subject"].to_string()).collect::<Vec<_>>().join(" "));
    assert!(
        comms.iter().all(|c| c["content"].is_null() && c["text_content"].is_null() && c["direction"] == json!("Received")),
        "headers only: no body anywhere"
    );
    assert!(!serde_json::to_string(&comms).unwrap().contains("SECRET"), "nor a file's name");
    assert_eq!(titles(&admin.timeline("Contact", jane).await, "email"), ["Spares quote"], "filed on Jane by her address, in any case");
    assert_eq!(titles(&admin.timeline("Lead", pat).await, "email"), ["Hello from Pat"]);
    // pulled again with one more message: the first are not made twice
    let more = json!({"id": "p4", "message_id": "mid-4@provider.test", "subject": "One more", "from": {"address": "jane@acme.test"}, "received_at": at(0)});
    k.save_connection_def(&admin.ctx, Some(id), &as_test(messages(more), events(24, "confirmed"), false)).await.unwrap();
    pull("messages").await;
    assert_eq!(admin.list("Communication", json!({})).await.len(), 4, "three were there; one more, none twice");
    assert_eq!(titles(&admin.timeline("Contact", jane).await, "email").len(), 2);

    // ── why events are not pulled: mapped onto Mail's CalendarEvent (fetched or not) they invite the guests again
    k.save_connection_def(&admin.ctx, Some(id), &as_test(messages(Value::Null), events(24, "confirmed"), true)).await.unwrap();
    pull("events").await;
    let invited = admin.list("Communication", json!({"direction": "Sent"})).await;
    assert!(
        invited.iter().any(|c| c["subject"].as_str().is_some_and(|s| s.starts_with("Invitation: Spares call")) && c["recipients"] == json!("jane@acme.test")),
        "{invited:?}"
    );

    // ── what the platform's pulls cannot do: a pull is run by the system, so a person's own account is not theirs to give
    let mut personal = shipped("crm_personal_mailbox")["description"].clone();
    personal["pulls"] = shared["pulls"].clone();
    let input: ConnectionInput =
        serde_json::from_value(json!({"name": "crm_personal_mailbox", "label": "Personal", "description": personal, "enabled": true})).unwrap();
    let listed = k.connection_defs(&admin.ctx).await.unwrap();
    let existing = listed.iter().find(|c| c["name"] == "crm_personal_mailbox").unwrap()["id"].as_str().unwrap().parse().unwrap();
    k.save_connection_def(&admin.ctx, Some(existing), &input).await.unwrap();
    k.pull_now(&admin.ctx, existing, "messages").await.unwrap();
    drain(k).await;
    let pulls = k.connection_pulls(&admin.ctx, existing).await.unwrap();
    let err = pulls.iter().find(|p| p["name"] == "messages").unwrap()["last_error"].as_str().unwrap_or_default().to_string();
    assert!(err.contains("each person's own"), "a pull is no person: {err}");
    let _ = tenant;
}

/// What a received mail looks like to CRM once Mail has kept it: its headers, as Mail keeps them.
async fn received(admin: &U, tenant: Uuid, sender: &str, subject: &str, headers: Value) {
    let sys = Ctx::system(tenant, admin.k.registry.schema(tenant).await.unwrap());
    admin
        .k
        .insert(
            &sys,
            "Communication",
            obj(json!({"channel": "Email", "direction": "Received", "status": "Received", "sender": sender, "recipients": "admin@mailbox.test", "subject": subject,
                       "communication_date": chrono::Utc::now().to_rfc3339(), "headers": headers})),
        )
        .await
        .unwrap_or_else(|e| panic!("received mail: {e:?}"));
    drain(&admin.k).await;
}

/// Sequences send through Mail (`outbound::send`): Reply-To the owner, threaded under the earlier ones, their Message-IDs
/// kept; a reply stops the sequence, an automatic answer does not, a bounce stops it and marks the address, an opt-out
/// is honoured before anything is sent, and an erasure takes the addresses kept.
async fn sequence_mail(admin: &U, tenant: Uuid, company: &Value) {
    let k = &admin.k;
    let template: ferrum_meta::MessageDef = serde_json::from_value(json!({
        "key": "x_seq_mail", "entity": "Lead", "category": "promotional",
        "languages": {"en": {"email": {"subject": "Hello {{ doc.lead_name }}", "body": "<p>Hello {{ doc.lead_name }}</p>"}}}
    }))
    .unwrap();
    k.save_message_template(&admin.ctx, &template).await.unwrap();
    let seq = uid(&admin
        .insert(
            "Sequence",
            json!({"sequence_name": "Through Mail", "applies_to": "Lead", "steps": [{"step_type": "Message", "after_days": 0, "message": "x_seq_mail"},
            {"step_type": "Message", "after_days": 0, "message": "x_seq_mail"}, {"step_type": "Task", "after_days": 5, "subject": "Later"}]}),
        )
        .await)
    .to_string();
    let lead = |name: &'static str| async move {
        uid(&admin
            .insert("Lead", json!({"first_name": name, "last_name": "Seq", "email": format!("{}@seq.test", name.to_lowercase()), "company": company}))
            .await)
    };
    let enrol = |l: Uuid| {
        let seq = seq.clone();
        async move { uid(&admin.insert("SequenceEnrolment", json!({"sequence": seq, "lead": l.to_string()})).await) }
    };
    let step = || async { ferrum_app_crm::maintenance(k).await.unwrap() };
    let sent_to = |address: &'static str| async move { admin.list("Communication", json!({"direction": "Sent", "recipients": address})).await };

    // ── sent through Mail: Reply-To the owner, the second threaded under the first, the Message-IDs kept
    let amy = lead("Amy").await;
    let amy_en = enrol(amy).await;
    step().await;
    let first = sent_to("amy@seq.test").await;
    assert_eq!(first.len(), 1, "{first:?}");
    assert_eq!(first[0]["reply_to_address"], json!("admin@mailbox.test"));
    assert_eq!((first[0]["reference_entity"].clone(), first[0]["reference_id"].clone()), (json!("Lead"), json!(amy.to_string())));
    assert!(first[0]["message_id"].is_string() && first[0]["in_reply_to"].is_null());
    step().await;
    let both = sent_to("amy@seq.test").await;
    let second = both.iter().find(|c| c["id"] != first[0]["id"]).expect("the second mail");
    assert_eq!(second["in_reply_to"], first[0]["message_id"], "threaded under the first");
    assert!(second["mail_references"].as_str().unwrap().contains(first[0]["message_id"].as_str().unwrap()));
    let kept: i64 = sqlx::query_scalar("select count(*) from crm_sequence_mail where enrolment = $1")
        .bind(amy_en)
        .fetch_one(&mut *k.db.system_tx().await.unwrap())
        .await
        .unwrap();
    assert_eq!(kept, 2);

    // ── an automatic answer is not a reply; their own answer, threaded or not, is
    received(admin, tenant, "amy@seq.test", "Out of office", json!({"auto-submitted": "auto-replied", "in-reply-to": first[0]["message_id"]})).await;
    received(admin, tenant, "amy@seq.test", "Newsletter", json!({"precedence": "bulk", "list-id": "<news.seq.test>"})).await;
    assert_eq!(admin.get("SequenceEnrolment", amy_en).await["status"], json!("Active"));
    received(admin, tenant, "amy@seq.test", "Re: Hello Amy", json!({"in-reply-to": second["message_id"], "references": first[0]["message_id"]})).await;
    let e = admin.get("SequenceEnrolment", amy_en).await;
    assert_eq!((e["status"].clone(), e["stopped_reason"].clone()), (json!("Stopped"), json!("Replied")));

    // ── a bounce stops it and marks the address: by the failed recipient, or by the message it answers
    let (bo, cy) = (lead("Bo").await, lead("Cy").await);
    let (bo_en, cy_en) = (enrol(bo).await, enrol(cy).await);
    step().await;
    let cy_mail = sent_to("cy@seq.test").await;
    received(admin, tenant, "mailer-daemon@mail.test", "Undelivered Mail", json!({"x-failed-recipients": "bo@seq.test", "content-type": "multipart/report"}))
        .await;
    received(admin, tenant, "mailer-daemon@mail.test", "Undelivered Mail", json!({"in-reply-to": cy_mail[0]["message_id"]})).await;
    for (l, en) in [(bo, bo_en), (cy, cy_en)] {
        let e = admin.get("SequenceEnrolment", en).await;
        assert_eq!((e["status"].clone(), e["stopped_reason"].clone()), (json!("Stopped"), json!("Bounced")));
        assert!(admin.get("Lead", l).await["email_bounced_on"].is_string(), "the address is marked");
    }
    let amy_replies = admin.get("Lead", amy).await;
    assert!(amy_replies["email_bounced_on"].is_null(), "a reply is no bounce");
    let other = admin
        .insert(
            "Sequence",
            json!({"sequence_name": "Another", "applies_to": "Lead", "steps": [{"step_type": "Message", "after_days": 0, "message": "x_seq_mail"}]}),
        )
        .await;
    let again = uid(&admin.insert("SequenceEnrolment", json!({"sequence": uid(&other).to_string(), "lead": bo.to_string()})).await);
    step().await;
    assert_eq!(admin.get("SequenceEnrolment", again).await["stopped_reason"], json!("Bounced"), "nothing more is sent to a bounced address");
    assert_eq!(sent_to("bo@seq.test").await.len(), 1);

    // ── an opt-out in the platform's list is honoured before anything is sent
    let dee = lead("Dee").await;
    k.set_message_opt_out(&admin.ctx, "dee@seq.test", "email", true).await.unwrap();
    let dee_en = enrol(dee).await;
    step().await;
    assert_eq!(admin.get("SequenceEnrolment", dee_en).await["stopped_reason"], json!("Opted Out"));
    assert!(sent_to("dee@seq.test").await.is_empty());

    // ── an erasure takes the addresses a sequence kept (the report names them)
    let found = k.personal_data(&admin.ctx, "amy@seq.test").await.unwrap();
    assert!(found.to_string().contains("a sequence sent them"), "{found}");
    k.erase_personal_data(&admin.ctx, "amy@seq.test", "Request 13").await.unwrap();
    let left: i64 = sqlx::query_scalar("select count(*) from crm_sequence_mail where lower(address) = 'amy@seq.test'")
        .fetch_one(&mut *k.db.system_tx().await.unwrap())
        .await
        .unwrap();
    assert_eq!(left, 0);
    let _ = tenant;
}
