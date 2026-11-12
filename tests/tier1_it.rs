//! The tier-1 gaps (12 Nov 2026) against a real PostgreSQL (`FERRUM_CRM_TEST_DATABASE_URL`): a quotation the
//! customer reads and answers on a signed link, one timeline for the people and deals, Mail's communications
//! and calendar events filed on the people they are with, sequences, forecasts you submit, and record types.
//!
//! Mail is not a dependency of CRM: `fixture` declares the few fields of its Communication, Event and
//! (Support's) Ticket that CRM reads, as Mail and Support declare them.

use ferrum_kernel::action_link::LinkSpec;
use ferrum_kernel::tenant::NewTenant;
use ferrum_kernel::{Ctx, Db, DbConfig, Doc, Error, HookRegistry, Kernel, KernelSettings, ListQuery};
use ferrum_meta::Catalog;
use rust_decimal::Decimal;
use serde_json::{Map, Value, json};
use std::str::FromStr;
use std::sync::Arc;
use uuid::Uuid;

const PASSWORD: &str = "Correct-Horse-9";

fn obj(v: Value) -> Doc {
    v.as_object().cloned().unwrap()
}
fn n(s: &str) -> Decimal {
    Decimal::from_str(s).unwrap()
}
fn dec(v: &Value) -> Decimal {
    match v {
        Value::String(s) => n(s),
        Value::Number(x) => n(&x.to_string()),
        _ => Decimal::ZERO,
    }
}
fn uid(doc: &Doc) -> Uuid {
    doc["id"].as_str().unwrap().parse().unwrap()
}
fn vid(v: &Value) -> Uuid {
    v["id"].as_str().unwrap().parse().unwrap()
}
fn given(v: Value) -> Map<String, Value> {
    v.as_object().cloned().unwrap()
}

static ZONE: std::sync::LazyLock<&str> =
    std::sync::LazyLock::new(|| if chrono::Timelike::hour(&chrono::Utc::now()) >= 11 { "Pacific/Kiritimati" } else { "Etc/GMT+12" });

fn today() -> chrono::NaiveDate {
    ferrum_kernel::ctx::local_now(&ZONE).date_naive()
}

fn refused<T: std::fmt::Debug>(r: Result<T, Error>, needle: &str) {
    match &r {
        Err(Error::Invalid(m)) | Err(Error::Forbidden(m)) | Err(Error::Conflict(m)) if m.contains(needle) => {}
        Err(Error::Validation(errs)) if errs.iter().any(|e| e.field == needle || e.message.contains(needle)) => {}
        Err(Error::NotFound(_)) if needle == "not found" => {}
        other => panic!("expected a refusal mentioning '{needle}', got {other:?}"),
    }
}

#[derive(Default)]
struct MemFiles(std::sync::Mutex<std::collections::HashMap<String, Vec<u8>>>);

#[async_trait::async_trait]
impl ferrum_kernel::files::FileStore for MemFiles {
    async fn put(&self, key: &str, data: &[u8]) -> ferrum_kernel::Result<()> {
        self.0.lock().unwrap().insert(key.to_string(), data.to_vec());
        Ok(())
    }
    async fn get(&self, key: &str) -> ferrum_kernel::Result<Vec<u8>> {
        self.0.lock().unwrap().get(key).cloned().ok_or_else(|| Error::NotFound("object".into()))
    }
    async fn delete(&self, key: &str) -> ferrum_kernel::Result<()> {
        self.0.lock().unwrap().remove(key);
        Ok(())
    }
}

/// A mailer that can attach files and keeps the mail it was given.
struct Captured;

static SENT: std::sync::Mutex<Vec<(String, String, String, Vec<String>)>> = std::sync::Mutex::new(Vec::new());

#[async_trait::async_trait]
impl ferrum_kernel::Mailer for Captured {
    async fn send(&self, to: &str, subject: &str, html: &str) -> ferrum_kernel::Result<()> {
        SENT.lock().unwrap().push((to.into(), subject.into(), html.into(), vec![]));
        Ok(())
    }
    fn can_attach(&self) -> bool {
        true
    }
    async fn send_with(&self, to: &str, subject: &str, html: &str, attachments: &[ferrum_kernel::MailAttachment]) -> ferrum_kernel::Result<()> {
        SENT.lock().unwrap().push((to.into(), subject.into(), html.into(), attachments.iter().map(|a| a.file_name.clone()).collect()));
        Ok(())
    }
}

/// What CRM reads of Mail's and Support's records, as they declare them.
fn fixture() -> Vec<ferrum_meta::Package> {
    let package = "package: comms\nversion: 0.1.0\nlabel: Mail fixture\ndepends: [core, parties]\nroles:\n  - name: Comms Reader\n    permissions:\n      - {entity: Communication, read: true}\n      - {entity: CalendarEvent, read: true}\n      - {entity: Ticket, read: true}\n";
    let entities = r#"
- name: Communication
  label: Communication
  kind: master
  fields:
    - {name: subject, type: data}
    - {name: channel, type: select, options: [Email, SMS, Chat, Phone, Note, Meeting], default: Email, required: true}
    - {name: direction, type: select, options: [Sent, Received], default: Sent, required: true}
    - {name: status, type: data}
    - {name: communication_date, type: datetime}
    - {name: sender, type: data}
    - {name: sender_name, type: data}
    - {name: recipients, type: text}
    - {name: cc, type: text}
    - {name: bcc, type: text}
    - {name: text_content, type: text}
    - {name: reference_entity, type: data}
    - {name: reference_id, type: data}
- name: CalendarEventAttendee
  label: Attendee
  kind: child
  fields:
    - {name: email, type: email, required: true}
- name: CalendarEvent
  label: Event
  kind: master
  fields:
    - {name: subject, type: data, required: true}
    - {name: starts_at, type: datetime, required: true}
    - {name: ends_at, type: datetime, required: true}
    - {name: status, type: select, options: [Confirmed, Tentative, Cancelled], default: Confirmed, required: true}
    - {name: owner, type: email}
    - {name: attendees, type: table, of: CalendarEventAttendee}
- name: Ticket
  label: Ticket
  kind: master
  fields:
    - {name: subject, type: data}
    - {name: status, type: data}
    - {name: priority, type: data}
    - {name: customer, type: link, to: Customer}
    - {name: contact_email, type: email}
    - {name: opened_at, type: datetime}
"#;
    ferrum_meta::catalog::load_embedded(&[("comms", "package.yaml", package), ("comms", "entities/fixture.yaml", entities)]).expect("fixture")
}

async fn fresh_kernel() -> Arc<Kernel> {
    let url = std::env::var("FERRUM_CRM_TEST_DATABASE_URL").unwrap_or_else(|_| "postgres://ferrum:ferrum@localhost:5432/ferrum_crm_test".into());
    let db = Db::connect(DbConfig { url, max_connections: 10, ..Default::default() }).await.expect("connect");
    sqlx::raw_sql("drop schema if exists public cascade; create schema public;").execute(&db.pool).await.expect("reset");
    let mut packages = ferrum_domain::packages().expect("platform packages");
    packages.extend(ferrum_app_erp::packages().expect("ERP's packages"));
    packages.extend(ferrum_app_crm::packages().expect("app packages"));
    packages.extend(fixture());
    let catalog = Catalog::from_packages(packages).expect("catalog");
    let mut hooks = HookRegistry::default();
    ferrum_domain::register(&mut hooks);
    ferrum_app_erp::register(&mut hooks);
    for source in ferrum_app_erp::reports() {
        ferrum_domain::reports::add_source(source);
    }
    ferrum_app_crm::register(&mut hooks);
    for source in ferrum_app_crm::reports() {
        ferrum_domain::reports::add_source(source);
    }
    let settings = KernelSettings {
        secret_key: Some(ferrum_kernel::secret::SecretKey::new([7u8; 32])),
        public_url: Some("https://ferrum.test".into()),
        ..Default::default()
    };
    let k = Kernel::boot(db, catalog, hooks, settings, Some(Arc::new(Captured))).await.expect("boot");
    k.set_file_store(Arc::new(MemFiles::default()));
    ferrum_domain::maintenance(&k).await.expect("maintenance");
    ferrum_app_erp::maintenance(&k).await.expect("maintenance");
    k
}

struct U {
    k: Arc<Kernel>,
    ctx: Ctx,
}

impl U {
    async fn insert(&self, entity: &str, v: Value) -> Doc {
        self.k.insert(&self.ctx, entity, obj(v)).await.unwrap_or_else(|e| panic!("insert {entity}: {e:?}"))
    }
    async fn update(&self, entity: &str, id: Uuid, v: Value) -> Doc {
        self.k.update(&self.ctx, entity, id, obj(v), None).await.unwrap_or_else(|e| panic!("update {entity}: {e:?}"))
    }
    async fn submit(&self, entity: &str, id: Uuid) -> Doc {
        self.k.submit(&self.ctx, entity, id, None).await.unwrap_or_else(|e| panic!("submit {entity}: {e:?}"))
    }
    async fn get(&self, entity: &str, id: Uuid) -> Doc {
        self.k.get(&self.ctx, entity, id, false).await.unwrap_or_else(|e| panic!("get {entity}: {e:?}"))
    }
    async fn action(&self, entity: &str, id: Uuid, action: &str, params: Value) -> Value {
        self.k.run_action(&self.ctx, entity, id, action, params).await.unwrap_or_else(|e| panic!("{entity}.{action}: {e:?}"))
    }
    async fn list(&self, entity: &str, filters: Value) -> Vec<Doc> {
        let q = ListQuery { filters: ferrum_kernel::query::parse_filters(&filters).unwrap(), limit: Some(500), ..Default::default() };
        self.k.list(&self.ctx, entity, &q).await.unwrap_or_else(|e| panic!("list {entity}: {e:?}")).rows
    }
    async fn one(&self, entity: &str, filters: Value) -> Doc {
        let rows = self.list(entity, filters.clone()).await;
        assert_eq!(rows.len(), 1, "{entity} {filters}");
        rows[0].clone()
    }
    async fn timeline(&self, entity: &str, id: Uuid, query: Value) -> Value {
        let q = ferrum_app_crm::timeline(&self.k, &self.ctx, entity, id, &given(query)).await;
        q.unwrap_or_else(|e| panic!("timeline {entity}: {e:?}"))
    }
}

async fn scalar<T: for<'r> sqlx::Decode<'r, sqlx::Postgres> + sqlx::Type<sqlx::Postgres> + Send + Unpin>(
    k: &Kernel,
    q: &str,
    tenant: Uuid,
    record: Option<Uuid>,
) -> T {
    let mut tx = k.db.system_tenant_tx(tenant).await.unwrap();
    let v = sqlx::query_scalar(sqlx::AssertSqlSafe(q.to_string())).bind(tenant).bind(record).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    v
}

/// The token of a link's address.
fn token(url: &str) -> String {
    url.rsplit('/').next().unwrap().to_string()
}

#[test]
fn tier1_end_to_end() {
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().thread_stack_size(16 << 20).build().unwrap();
    rt.block_on(async { tokio::spawn(scenario()).await.unwrap() });
}

async fn scenario() {
    let k = fresh_kernel().await;
    let tenant = k
        .create_tenant(NewTenant {
            slug: "tier1".into(),
            name: "Tier One Ltd".into(),
            admin_email: "admin@tier1.test".into(),
            admin_password: PASSWORD.into(),
            admin_name: None,
            packages: vec!["crm".into(), "crm_erp".into(), "comms".into()],
            settings: Some(json!({"fiscal_year_start_month": 4, "timezone": *ZONE})),
        })
        .await
        .expect("tenant");
    let session = k.login("tier1", "admin@tier1.test", PASSWORD).await.expect("login");
    let admin = U { k: k.clone(), ctx: k.load_ctx(tenant, session.user, Some(session.token_version), "it".into()).await.expect("ctx") };
    let inr = admin.one("Currency", json!({"code": "INR"})).await;
    let company = admin.insert("Company", json!({"company_name": "Tier One Ltd", "abbr": "TO", "default_currency": inr["id"]})).await;
    let cid = uid(&company).to_string();
    let output_tax = admin.one("Account", json!({"account_name": "Output Tax", "company": cid})).await;
    admin
        .insert(
            "TaxTemplate",
            json!({"title": "VAT 10%", "company": cid, "kind": "Sales", "is_default": true,
                   "rows": [{"charge_type": "On Net Total", "account": output_tax["id"], "description": "VAT 10%", "rate": "10"}]}),
        )
        .await;
    let products = admin.one("ItemGroup", json!({"group_name": "Products"})).await;
    let nos = admin.one("Uom", json!({"uom_name": "Nos"})).await;
    let widget = uid(&admin
        .insert("Item", json!({"item_code": "WIDGET", "item_name": "Widget", "item_group": products["id"], "stock_uom": nos["id"], "standard_rate": "150"}))
        .await)
    .to_string();
    let user = |email: &'static str, name: &'static str, roles: &'static [&'static str]| {
        let (k, admin_ctx) = (k.clone(), admin.ctx.clone());
        async move {
            let roles: Vec<String> = roles.iter().map(|r| r.to_string()).collect();
            let id = k.create_user(&admin_ctx, email, name, Some(PASSWORD), &roles, false).await.expect("user");
            U { ctx: k.load_ctx(tenant, id, None, "it".into()).await.expect("user ctx"), k }
        }
    };
    let anna = user("anna@tier1.test", "Anna Rep", &["Sales User", "Comms Reader"]).await;
    let ben = user("ben@tier1.test", "Ben Rep", &["Sales User"]).await;
    let maria = user("maria@tier1.test", "Maria Manager", &["Sales Manager", "Comms Reader"]).await;
    let team = maria
        .insert(
            "SalesTeam",
            json!({"team_name": "North", "manager": "maria@tier1.test", "members": [{"person": "anna@tier1.test"}, {"person": "ben@tier1.test"}]}),
        )
        .await;

    let (acme, jane) = quotation_link(&k, &admin, &anna, &ben, &cid, &widget, tenant).await;
    sent_by_0_12_0(&k, &admin, &anna, acme, jane, &widget, tenant).await;
    timeline_and_filing(&k, &admin, &anna, &ben, &maria, (acme, jane), &cid, tenant).await;
    hosted_mail(&k, &admin, &anna, tenant).await;
    sequences(&k, &admin, &anna, tenant).await;
    forecast(&k, &admin, &anna, &ben, &maria, tenant).await;
    record_types(&admin, &anna).await;
    let _ = &team;
}

async fn customer(admin: &U, name: &str, email: Option<&str>) -> Uuid {
    uid(&admin.insert("Customer", json!({"customer_name": name, "email": email})).await)
}

/// A submitted quotation of `qty` widgets for the customer, owned by `who`.
async fn quote(who: &U, customer: Uuid, contact: Option<Uuid>, widget: &str, qty: &str) -> Uuid {
    let q = who.insert("Quotation", json!({"party_type": "Customer", "customer": customer.to_string(), "contact": contact.map(|c| c.to_string()), "items": [{"item": widget, "qty": qty}]})).await;
    let q = uid(&q);
    who.submit("Quotation", q).await;
    q
}

async fn quotation_link(k: &Arc<Kernel>, admin: &U, anna: &U, ben: &U, cid: &str, widget: &str, tenant: Uuid) -> (Uuid, Uuid) {
    let _ = cid;
    let acme = customer(admin, "Acme Corp", Some("buyer@acme.test")).await;
    let jane = uid(&admin
        .insert(
            "Contact",
            json!({"first_name": "Jane", "last_name": "Lee", "emails": [{"email": "jane@acme.test", "is_primary": true}], "customer": acme.to_string()}),
        )
        .await);
    let nobody = customer(admin, "No Address Ltd", None).await;

    let q1 = quote(anna, acme, Some(jane), widget, "10").await;
    let doc = anna.get("Quotation", q1).await;
    assert_eq!(doc["status"], json!("Open"));
    assert!(doc.get("lines_text").is_none_or(Value::is_null), "the hidden lines text is gone: the page shows the lines by columns");

    // ── nowhere to send it: refused, and nothing is sent
    let bare = quote(anna, nobody, None, widget, "1").await;
    refused(anna.k.run_action(&anna.ctx, "Quotation", bare, "send_quotation", json!({})).await, "no address");
    assert_eq!(anna.get("Quotation", bare).await["status"], json!("Open"));

    // ── sending: Sent, the document attached, one link, the message queued, in the history
    anna.action("Quotation", q1, "send_quotation", json!({})).await;
    let doc = anna.get("Quotation", q1).await;
    assert_eq!(doc["status"], json!("Sent"));
    assert_eq!(doc["sent_on"], json!(today().to_string()));
    let files: i64 = scalar(k, "select count(*) from file_object where tenant_id = $1 and entity = 'Quotation' and record_id = $2", tenant, Some(q1)).await;
    assert!(files >= 1, "the quotation is attached as a generated document");
    let links = anna.k.record_links(&anna.ctx, "Quotation", q1).await.unwrap();
    assert_eq!(links.len(), 1, "{links:?}");
    let l = &links[0];
    assert_eq!(l["actions"], json!(["customer_accept", "customer_decline", "customer_ask"]));
    assert!(l["show"].as_array().unwrap().iter().any(|f| f == "items"), "lines and totals by default");
    assert_eq!(l["columns"], json!({"items": ["item_name", "qty", "rate", "amount"]}));
    // it works through the quotation's valid-until (the platform's `until`), not a fixed 90 days
    let valid: chrono::NaiveDate = doc["valid_till"].as_str().unwrap().parse().unwrap();
    let ends = chrono::DateTime::parse_from_rfc3339(l["expires_at"].as_str().unwrap()).unwrap();
    assert_eq!(ends.date_naive(), valid + chrono::Duration::days(1), "through the whole of its last day: {l}");
    assert_eq!(l["max_uses"], json!(20));
    assert_eq!(l["code"], json!(false));
    assert_eq!(l["recipient"], json!("Acme Corp"));
    let sends = anna.k.message_log(&anna.ctx, Some(("Quotation", q1)), None, 10).await.unwrap();
    assert!(sends.iter().any(|s| s["template"] == "crm.quotation_sent" && s["recipient"] == "jane@acme.test"), "to the contact: {sends:?}");

    // ── sending again: the old link stops, one new
    anna.action("Quotation", q1, "send_quotation", json!({"to": "other@acme.test"})).await;
    let links = anna.k.record_links(&anna.ctx, "Quotation", q1).await.unwrap();
    assert_eq!(links.len(), 2);
    assert_eq!(links.iter().filter(|l| l["revoked_at"].is_null()).count(), 1, "only the newest works: {links:?}");
    let sends = anna.k.message_log(&anna.ctx, Some(("Quotation", q1)), None, 10).await.unwrap();
    assert!(sends.iter().any(|s| s["recipient"] == "other@acme.test"));

    // ── what the customer does, on a link of their own (the mailed one's address is kept nowhere)
    let spec = |uses| LinkSpec {
        actions: vec!["customer_accept".into(), "customer_decline".into(), "customer_ask".into()],
        show: vec!["party_name".into(), "valid_till".into(), "items".into(), "grand_total".into()],
        columns: [("items".to_string(), vec!["item_name".to_string(), "qty".to_string(), "rate".to_string(), "amount".to_string()])].into(),
        uses: Some(uses),
        recipient: Some("Acme Corp".into()),
        ..Default::default()
    };
    let link = admin.k.issue_link_now(&admin.ctx, "Quotation", q1, &spec(20)).await.unwrap();
    let t = token(&link.url);
    let page = admin.k.link_page("tier1", &t).await.unwrap().expect("the page");
    assert_eq!(page.tables[0].2[0][..2], ["Widget".to_string(), "10".to_string()], "the customer reads the lines: {:?}", page.tables);
    assert_eq!(page.actions.iter().map(|a| a.0.as_str()).collect::<Vec<_>>(), ["customer_accept", "customer_decline", "customer_ask"]);

    // a question lands as a task for the owner; the quotation is Viewed once the platform says the page was acted on
    assert_eq!(anna.get("Quotation", q1).await["status"], json!("Sent"), "reading the page alone views nothing");
    k.link_opened("tier1", &t).await.unwrap();
    k.use_link("tier1", &t, "customer_ask", &given(json!({"question": "Can you ship by Friday?", "signed_name": "Jane Lee"})), None)
        .await
        .unwrap()
        .expect("asked");
    let task = anna.one("Activity", json!({"subject": "Can you ship by Friday?"})).await;
    assert_eq!(task["assigned_to"], json!("anna@tier1.test"));
    assert_eq!(task["activity_type"], json!("Task"));
    assert_eq!(task["customer"], json!(acme.to_string()));
    assert!(task["details"].as_str().unwrap().contains("Jane Lee"));
    drain(k).await;
    let doc = anna.get("Quotation", q1).await;
    assert_eq!(doc["status"], json!("Viewed"));
    assert!(doc["viewed_at"].is_string());
    let notes = admin.k.message_log(&admin.ctx, Some(("Quotation", q1)), None, 20).await.unwrap();
    assert!(notes.iter().any(|s| s["template"] == "crm.quotation_question"), "its owner is told");

    // accepting needs the name typed
    let none = k.use_link("tier1", &t, "customer_accept", &Map::new(), None).await;
    assert!(none.is_err(), "the name is required: {none:?}");
    k.use_link("tier1", &t, "customer_accept", &given(json!({"signed_name": "Jane Lee"})), None).await.unwrap().expect("accepted");
    let doc = anna.get("Quotation", q1).await;
    assert_eq!(doc["status"], json!("Accepted"));
    assert_eq!(doc["answer"], json!("Accepted"));
    assert_eq!(doc["answer_name"], json!("Jane Lee"));
    assert_eq!(doc["answered_on"], json!(today().to_string()));
    assert!(doc["answered_by"].is_null(), "a person of the workspace did not record it");
    assert!(
        k.use_link("tier1", &t, "customer_accept", &given(json!({"signed_name": "Mallory"})), None).await.unwrap().is_none(),
        "an answered quotation's links are gone"
    );
    let trail = anna.k.audit_trail(&anna.ctx, "Quotation", q1).await.unwrap();
    let actions: Vec<&str> = trail["events"].as_array().unwrap().iter().filter_map(|e| e["action"].as_str()).collect();
    assert!(actions.contains(&"link:customer_accept") && actions.contains(&"link:customer_ask"), "who and when is in the history: {actions:?}");

    // ── declining: a reason is required; the answer is Rejected, in the customer's words
    let q2 = quote(anna, acme, Some(jane), widget, "2").await;
    anna.action("Quotation", q2, "send_quotation", json!({})).await;
    let t2 = token(&admin.k.issue_link_now(&admin.ctx, "Quotation", q2, &spec(5)).await.unwrap().url);
    assert!(k.use_link("tier1", &t2, "customer_decline", &Map::new(), None).await.is_err(), "the reason is required");
    assert!(k.use_link("tier1", &t2, "mark_lost", &given(json!({"lost_reason": "x"})), None).await.is_err(), "a link does only what it names");
    k.use_link("tier1", &t2, "customer_decline", &given(json!({"reason": "Too dear this quarter"})), None).await.unwrap().expect("declined");
    let doc = anna.get("Quotation", q2).await;
    assert_eq!(
        (doc["status"].clone(), doc["answer"].clone(), doc["answer_detail"].clone()),
        (json!("Rejected"), json!("Rejected"), json!("Too dear this quarter"))
    );
    refused(anna.k.run_action(&anna.ctx, "Quotation", q2, "send_quotation", json!({})).await, "not available");

    // ── what the page shows is the seller's choice
    let settings = admin.insert("CrmSettings", json!({"quote_link_shows": "Totals Only"})).await;
    let q3 = quote(anna, acme, Some(jane), widget, "3").await;
    anna.action("Quotation", q3, "send_quotation", json!({})).await;
    let shown = anna.k.record_links(&anna.ctx, "Quotation", q3).await.unwrap()[0]["show"].clone();
    assert!(shown.as_array().unwrap().iter().any(|f| f == "grand_total") && !shown.as_array().unwrap().iter().any(|f| f == "items"), "{shown}");
    admin.update("CrmSettings", uid(&settings), json!({"quote_link_shows": "Neither"})).await;
    anna.action("Quotation", q3, "send_quotation", json!({})).await;
    let shown = anna.k.record_links(&anna.ctx, "Quotation", q3).await.unwrap()[0]["show"].clone();
    assert_eq!(shown, json!(["party_name", "posting_date", "valid_till"]));

    // ── a one-time code, when the seller asks for it
    admin.update("CrmSettings", uid(&settings), json!({"quote_link_shows": "Lines and Totals", "quote_link_code": true})).await;
    anna.action("Quotation", q3, "send_quotation", json!({})).await;
    assert_eq!(anna.k.record_links(&anna.ctx, "Quotation", q3).await.unwrap()[0]["code"], json!(true));

    // ── the link ends with the quotation: expired, then cancelled for a revision
    let q4 = quote(anna, acme, Some(jane), widget, "4").await;
    anna.action("Quotation", q4, "send_quotation", json!({})).await;
    let t4 = token(&admin.k.issue_link_now(&admin.ctx, "Quotation", q4, &spec(5)).await.unwrap().url);
    sql(k, "update e_quotation set valid_till = current_date - 2 where tenant_id = $1 and id = $2", tenant, q4).await;
    assert!(k.use_link("tier1", &t4, "customer_accept", &given(json!({"signed_name": "Late"})), None).await.is_err(), "past its validity nothing is accepted");
    ferrum_app_crm::maintenance(k).await.unwrap();
    assert_eq!(anna.get("Quotation", q4).await["status"], json!("Expired"));
    assert!(anna.k.record_links(&anna.ctx, "Quotation", q4).await.unwrap().iter().all(|l| !l["revoked_at"].is_null()), "its links are revoked");
    let q5 = quote(anna, acme, Some(jane), widget, "5").await;
    anna.action("Quotation", q5, "send_quotation", json!({})).await;
    let t5 = token(&admin.k.issue_link_now(&admin.ctx, "Quotation", q5, &spec(5)).await.unwrap().url);
    anna.k.cancel(&anna.ctx, "Quotation", q5, None).await.unwrap();
    assert!(
        k.use_link("tier1", &t5, "customer_accept", &given(json!({"signed_name": "Old"})), None).await.unwrap().is_none(),
        "a cancelled (revised) quotation's link is gone"
    );
    let revised = anna.k.amend(&anna.ctx, "Quotation", q5).await.unwrap();
    assert_eq!(revised["revision"], json!(2));
    assert!(anna.k.record_links(&anna.ctx, "Quotation", q5).await.unwrap().iter().all(|l| !l["revoked_at"].is_null()));

    // ── the setting that orders what the customer accepts
    admin.update("CrmSettings", uid(&settings), json!({"quote_link_code": false, "order_on_accept": true})).await;
    let q6 = quote(anna, acme, Some(jane), widget, "6").await;
    anna.action("Quotation", q6, "send_quotation", json!({})).await;
    let t6 = token(&admin.k.issue_link_now(&admin.ctx, "Quotation", q6, &spec(5)).await.unwrap().url);
    k.use_link("tier1", &t6, "customer_accept", &given(json!({"signed_name": "Jane Lee"})), None).await.unwrap().expect("accepted");
    let order = admin.one("SalesOrder", json!({"crm_quotation": q6.to_string()})).await;
    assert_eq!(order["customer"], json!(acme.to_string()));
    assert_eq!(dec(&admin.get("SalesOrder", uid(&order)).await["items"][0]["qty"]), n("6"));
    // and without the setting an accepted quotation waits for a person, who orders it in one action
    admin.update("CrmSettings", uid(&settings), json!({"order_on_accept": false})).await;
    let t1b = token(&admin.k.issue_link_now(&admin.ctx, "Quotation", q1, &spec(5)).await.unwrap().url);
    assert!(k.use_link("tier1", &t1b, "customer_accept", &given(json!({"signed_name": "Again"})), None).await.is_err(), "the earlier answer stands");
    assert!(admin.list("SalesOrder", json!({"crm_quotation": q1.to_string()})).await.is_empty());
    let made = anna.action("Quotation", q1, "make_sales_order", json!({})).await;
    assert_eq!(made["entity"], json!("SalesOrder"));
    // two people on one link answering at once: one answer stands (the quotation is locked as the action runs)
    admin.update("CrmSettings", uid(&settings), json!({"order_on_accept": true})).await;
    let q7 = quote(anna, acme, Some(jane), widget, "7").await;
    anna.action("Quotation", q7, "send_quotation", json!({})).await;
    let t7 = token(&admin.k.issue_link_now(&admin.ctx, "Quotation", q7, &spec(20)).await.unwrap().url);
    let (a, b) = (given(json!({"signed_name": "Jane Lee"})), given(json!({"signed_name": "Jane Two"})));
    let (r1, r2) = tokio::join!(k.use_link("tier1", &t7, "customer_accept", &a, None), k.use_link("tier1", &t7, "customer_accept", &b, None));
    assert_eq!([&r1, &r2].iter().filter(|r| matches!(r, Ok(Some(_)))).count(), 1, "{r1:?} {r2:?}");
    assert_eq!(admin.list("SalesOrder", json!({"crm_quotation": q7.to_string()})).await.len(), 1, "one answer, one order");
    admin.update("CrmSettings", uid(&settings), json!({"order_on_accept": false})).await;
    let _ = ben;
    (acme, jane)
}

/// A quotation sent by 0.12.0, whose link (90 days, showing the hidden Lines Text) is in the customer's mail, after
/// this release's startup has run its migration: the link still shows the lines, still works, is seen as opened,
/// still ends with the quotation's validity (the maintenance, since it has no `until`) and when it is cancelled.
async fn sent_by_0_12_0(k: &Arc<Kernel>, admin: &U, anna: &U, acme: Uuid, jane: Uuid, widget: &str, tenant: Uuid) {
    let old_link = |q: Uuid| {
        let spec = LinkSpec {
            actions: vec!["customer_accept".into(), "customer_decline".into(), "customer_ask".into()],
            show: vec!["party_name".into(), "valid_till".into(), "grand_total".into()],
            uses: Some(20),
            recipient: Some("Acme Corp".into()),
            ..Default::default()
        };
        async move { token(&admin.k.issue_link_now(&admin.ctx, "Quotation", q, &spec).await.unwrap().url) }
    };
    let as_it_was = "update action_link set show = array['party_name', 'valid_till', 'lines_text', 'grand_total'], columns = '{}'::jsonb, \
                     expires_at = now() + interval '90 days' where tenant_id = $1 and record_id = $2";
    let migrate = |what: &'static str| async move {
        let mut tx = k.db.system_tenant_tx(tenant).await.unwrap();
        sqlx::raw_sql(what).execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
    };
    let migration = include_str!("../packages/crm/sql/008_link_lines.sql");

    let q = quote(anna, acme, Some(jane), widget, "8").await;
    anna.action("Quotation", q, "send_quotation", json!({})).await;
    let t = old_link(q).await;
    sql(k, as_it_was, tenant, q).await;
    migrate(migration).await;
    migrate(migration).await; // and again: nothing moves twice
    let links = anna.k.record_links(&anna.ctx, "Quotation", q).await.unwrap();
    assert!(links.iter().any(|l| l["show"] == json!(["party_name", "valid_till", "items", "grand_total"]) && l["columns"]["items"].is_array()), "{links:?}");
    let page = k.link_page("tier1", &t).await.unwrap().expect("the old link still works");
    assert_eq!(page.tables[0].2[0][..2], ["Widget".to_string(), "8".to_string()], "and still shows the lines: {:?}", page.tables);
    k.link_opened("tier1", &t).await.unwrap();
    k.use_link("tier1", &t, "customer_ask", &given(json!({"question": "Is stock there?", "signed_name": "Jane Lee"})), None).await.unwrap().expect("asked");
    drain(k).await;
    assert_eq!(anna.get("Quotation", q).await["status"], json!("Viewed"));
    // sent again: the old link's later opening says nothing (it is ended), and a fresh quotation's link is not affected
    anna.action("Quotation", q, "send_quotation", json!({})).await;
    assert_eq!(anna.get("Quotation", q).await["status"], json!("Sent"));
    assert!(k.link_page("tier1", &t).await.unwrap().is_none(), "ended by the new send");
    // its validity passes: the maintenance ends the 90-day link the new platform call would not know of
    let t2 = old_link(q).await;
    sql(k, as_it_was, tenant, q).await;
    sql(k, "update e_quotation set valid_till = current_date - 2 where tenant_id = $1 and id = $2", tenant, q).await;
    ferrum_app_crm::maintenance(k).await.unwrap();
    assert_eq!(anna.get("Quotation", q).await["status"], json!("Expired"));
    assert!(k.use_link("tier1", &t2, "customer_accept", &given(json!({"signed_name": "Late"})), None).await.unwrap().is_none(), "expired with its quotation");
    // cancelled for a revision: revoked by the platform's call, recorded in the history
    let q = quote(anna, acme, Some(jane), widget, "9").await;
    anna.action("Quotation", q, "send_quotation", json!({})).await;
    let t3 = old_link(q).await;
    sql(k, as_it_was, tenant, q).await;
    migrate(migration).await;
    anna.k.cancel(&anna.ctx, "Quotation", q, None).await.unwrap();
    assert!(k.use_link("tier1", &t3, "customer_accept", &given(json!({"signed_name": "Old"})), None).await.unwrap().is_none());
    let trail = anna.k.audit_trail(&anna.ctx, "Quotation", q).await.unwrap();
    assert!(trail["events"].as_array().unwrap().iter().any(|e| e["action"] == "revoke_link"), "the platform says so in the history");
}

async fn sql(k: &Kernel, q: &str, tenant: Uuid, record: Uuid) {
    let mut tx = k.db.system_tenant_tx(tenant).await.unwrap();
    sqlx::query(sqlx::AssertSqlSafe(q.to_string())).bind(tenant).bind(record).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
}

async fn drain(k: &Kernel) {
    while k.dispatch_outbox_once().await.expect("dispatch") > 0 {}
}

fn kinds(t: &Value) -> Vec<String> {
    t["events"].as_array().unwrap().iter().map(|e| e["kind"].as_str().unwrap().to_string()).collect()
}

fn titles(t: &Value, kind: &str) -> Vec<String> {
    t["events"].as_array().unwrap().iter().filter(|e| e["kind"] == kind).filter_map(|e| e["title"].as_str().map(str::to_string)).collect()
}

async fn mail(admin: &U, direction: &str, from: &str, to: &str, subject: &str, channel: &str) -> Uuid {
    // As Mail keeps a header: the address, and the name beside it.
    let (name, from) = match (from.find('<'), from.find('>')) {
        (Some(a), Some(b)) => (Some(from[..a].trim().trim_matches('"').to_string()), from[a + 1..b].to_string()),
        _ => (None, from.to_string()),
    };
    let from = from.as_str();
    uid(&admin
        .insert(
            "Communication",
            json!({"subject": subject, "channel": channel, "direction": direction, "sender": from, "sender_name": name, "recipients": to,
                   "communication_date": chrono::Utc::now().to_rfc3339(), "text_content": format!("About {subject}")}),
        )
        .await)
}

#[allow(clippy::too_many_arguments)]
async fn timeline_and_filing(k: &Arc<Kernel>, admin: &U, anna: &U, ben: &U, maria: &U, (acme, jane): (Uuid, Uuid), cid: &str, tenant: Uuid) {
    let _ = cid;
    let settings = admin.one("CrmSettings", json!({})).await;
    let set = |v: Value| {
        let (admin, id) = (admin, uid(&settings));
        async move { admin.update("CrmSettings", id, v).await }
    };
    let pat = uid(&anna
        .insert("Lead", json!({"first_name": "Pat", "last_name": "Prospect", "email": "Pat@Lead.test", "phone": "+91 98450 00001", "source": "Website"}))
        .await);
    let stages = admin.list("SalesStage", json!({})).await;
    let stage = |name: &str| stages.iter().find(|s| s["stage_name"] == json!(name)).map(|s| s["id"].clone()).unwrap();
    // Two open deals of Acme: the older one names Jane; the newer one names nobody. And Pat's.
    let d1 = uid(&anna
        .insert(
            "Opportunity",
            json!({"opportunity_from": "Customer", "customer": acme.to_string(), "contact": jane.to_string(), "title": "Acme spares", "amount": "500"}),
        )
        .await);
    let d2 =
        uid(&anna.insert("Opportunity", json!({"opportunity_from": "Customer", "customer": acme.to_string(), "title": "Acme service", "amount": "900"})).await);
    let d3 = uid(&anna.insert("Opportunity", json!({"opportunity_from": "Lead", "lead": pat.to_string(), "title": "Pat's deal", "amount": "100"})).await);
    anna.update("Opportunity", d1, json!({"sales_stage": stage("Proposal")})).await;

    // ── mail: the other party's exact address; one record, and the newest open deal by the stated rule
    mail(admin, "Received", "\"Jane Lee\" <JANE@acme.test>", "anna@tier1.test", "Spares quote", "Email").await;
    mail(admin, "Sent", "anna@tier1.test", "pat@lead.test", "Welcome Pat", "Email").await;
    mail(admin, "Received", "buyer@acme.test", "anna@tier1.test", "Our account", "Email").await;
    mail(admin, "Sent", "anna@tier1.test", "ben@tier1.test", "Internal only", "Email").await;
    mail(admin, "Received", "pat@other.test", "anna@tier1.test", "A Pat by name only", "Email").await;
    mail(admin, "Received", "zed@new.test", "anna@tier1.test", "Hello from Zed", "Email").await;
    mail(admin, "Sent", "anna@tier1.test", "098450 00001", "Called Pat", "Phone").await;
    mail(admin, "Sent", "anna@tier1.test", "+1 555 0100", "Called nobody", "Phone").await;
    drain(k).await;
    assert!(admin.list("Lead", json!({"email": "zed@new.test"})).await.is_empty(), "an unknown sender is no lead unless the setting says so");
    let t = anna.timeline("Contact", jane, json!({})).await;
    assert_eq!(titles(&t, "email"), ["Spares quote"], "Jane's mail, her address matched in any case");
    let t = anna.timeline("Lead", pat, json!({})).await;
    assert_eq!(titles(&t, "email"), ["Welcome Pat"], "a mail Pat was sent; a Pat by name only is nobody");
    assert_eq!(titles(&t, "call"), ["Called Pat"], "a call, by the number's last ten digits");
    let t = anna.timeline("Opportunity", d1, json!({})).await;
    assert_eq!(titles(&t, "email"), ["Spares quote"], "Jane's newest open deal that names her, not the newer one of her company");
    assert!(
        titles(&anna.timeline("Opportunity", d2, json!({})).await, "email").contains(&"Our account".to_string()),
        "the company's own address: its newest open deal"
    );
    assert_eq!(
        titles(&anna.timeline("Opportunity", d3, json!({})).await, "email"),
        ["Welcome Pat", "Called Pat"].iter().filter(|t| **t == "Welcome Pat").map(|t| t.to_string()).collect::<Vec<_>>()
    );
    let t = anna.timeline("Customer", acme, json!({})).await;
    assert!(titles(&t, "email").contains(&"Our account".to_string()), "{t}");
    assert!(!titles(&t, "email").contains(&"Internal only".to_string()));
    assert_eq!(anna.get("Lead", pat).await["status"], json!("New"), "mail does not move a lead's status");
    assert!(anna.get("Lead", pat).await["last_touched_at"].is_string(), "but it counts as touched");

    // ── an unknown sender becomes a lead only by the setting; automatic senders never do
    set(json!({"unknown_sender_lead": true})).await;
    mail(admin, "Received", "\"Zed Zimmer\" <zed@new.test>", "anna@tier1.test", "Hello again", "Email").await;
    mail(admin, "Received", "no-reply@news.test", "anna@tier1.test", "Newsletter", "Email").await;
    mail(admin, "Sent", "anna@tier1.test", "ben@tier1.test", "Still internal", "Email").await;
    drain(k).await;
    let zed = admin.one("Lead", json!({"email": "zed@new.test"})).await;
    assert_eq!((zed["first_name"].clone(), zed["last_name"].clone(), zed["source"].clone()), (json!("Zed"), json!("Zimmer"), json!("Email")));
    assert!(admin.list("Lead", json!({"email": "no-reply@news.test"})).await.is_empty());
    assert_eq!(titles(&admin.timeline("Lead", uid(&zed), json!({})).await, "email"), ["Hello again"]);
    mail(admin, "Received", "zed@new.test", "anna@tier1.test", "Zed again", "Email").await;
    drain(k).await;
    assert_eq!(admin.list("Lead", json!({"email": "zed@new.test"})).await.len(), 1, "the second mail finds the lead the first made");
    set(json!({"unknown_sender_lead": false})).await;

    // ── the same event twice files once
    sql(
        k,
        "update outbox set processed_at = null, available_at = now() where tenant_id = $1 and record_id = $2 and topic = 'Communication.created'",
        tenant,
        uid(&zed),
    )
    .await;
    drain(k).await;

    // ── the timeline: newest first, filtered by kind, in pages, only what the viewer may read
    let t = anna.timeline("Customer", acme, json!({})).await;
    let at: Vec<chrono::DateTime<chrono::FixedOffset>> =
        t["events"].as_array().unwrap().iter().map(|e| chrono::DateTime::parse_from_rfc3339(e["at"].as_str().unwrap()).unwrap()).collect();
    assert!(at.windows(2).all(|w| w[0] >= w[1]), "newest first: {at:?}");
    let quotation: Vec<String> =
        t["events"].as_array().unwrap().iter().filter(|e| e["kind"] == "quotation").map(|e| e["details"]["event"].as_str().unwrap().to_string()).collect();
    for want in ["created", "sent", "viewed", "accepted", "rejected"] {
        assert!(quotation.contains(&want.to_string()), "quotation {want} on the customer's timeline: {quotation:?}");
    }
    assert!(kinds(&t).contains(&"order".to_string()), "the order made from a quotation");
    let only = anna.timeline("Customer", acme, json!({"kinds": "quotation"})).await;
    assert!(kinds(&only).iter().all(|x| x == "quotation") && !kinds(&only).is_empty());
    refused(ferrum_app_crm::timeline(k, &anna.ctx, "Customer", acme, &given(json!({"kinds": "gossip"}))).await, "kinds");
    let first = anna.timeline("Customer", acme, json!({"limit": 2})).await;
    assert_eq!(first["events"].as_array().unwrap().len(), 2);
    let next = &first["next"];
    let second = anna.timeline("Customer", acme, json!({"limit": 2, "before": next["before"], "before_id": next["before_id"]})).await;
    let ids = |v: &Value| v["events"].as_array().unwrap().iter().map(|e| e["id"].as_str().unwrap().to_string()).collect::<Vec<_>>();
    assert!(ids(&first).iter().all(|i| !ids(&second).contains(i)), "a page continues where the last stopped");
    let all = ids(&t);
    assert_eq!([ids(&first), ids(&second)].concat(), all[..4], "and in the same order");
    // a deal's stage changes, by the names of the stages
    let t = anna.timeline("Opportunity", d1, json!({"kinds": "stage"})).await;
    let last = &t["events"][0];
    assert_eq!(last["details"]["to"], json!("Proposal"));
    assert!(last["details"]["from"].is_string());
    // Support's tickets: through the customer, and for a contact by their own address
    let ticket = |v: Value| async move { uid(&admin.insert("Ticket", v).await) };
    ticket(json!({"subject": "Printer down", "customer": acme.to_string(), "contact_email": "buyer@acme.test", "opened_at": chrono::Utc::now().to_rfc3339()}))
        .await;
    ticket(json!({"subject": "Login help", "customer": acme.to_string(), "contact_email": "jane@acme.test", "opened_at": chrono::Utc::now().to_rfc3339()}))
        .await;
    assert_eq!(titles(&anna.timeline("Customer", acme, json!({})).await, "ticket").len(), 2);
    assert_eq!(titles(&anna.timeline("Contact", jane, json!({})).await, "ticket"), ["Login help"]);

    // ── rights: what the viewer may not read is not there; a record they may not open is not found
    refused(ben.k.get(&ben.ctx, "Lead", pat, false).await, "not found");
    refused(ferrum_app_crm::timeline(k, &ben.ctx, "Lead", pat, &Map::new()).await, "not found");
    let sam = {
        let id = k.create_user(&admin.ctx, "sam@tier1.test", "Sam Rep", Some(PASSWORD), &["Sales User".to_string()], false).await.unwrap();
        U { ctx: k.load_ctx(tenant, id, None, "it".into()).await.unwrap(), k: k.clone() }
    };
    let sams = uid(&sam.insert("Lead", json!({"first_name": "Sam", "last_name": "Lead", "email": "samlead@x.test"})).await);
    sam.insert("Activity", json!({"activity_type": "Note", "subject": "Sam's own note", "lead": sams.to_string()})).await;
    mail(admin, "Received", "samlead@x.test", "sam@tier1.test", "Hidden from Sam", "Email").await;
    drain(k).await;
    let t = sam.timeline("Lead", sams, json!({})).await;
    assert_eq!(titles(&t, "note"), ["Sam's own note"]);
    assert!(titles(&t, "email").is_empty(), "no right to read Mail's communications: none shown");
    assert_eq!(titles(&admin.timeline("Lead", sams, json!({})).await, "email"), ["Hidden from Sam"]);
    let _ = maria;

    // ── a meeting: a guest who is a known contact; it follows the event when it moves and when it goes
    let soon = chrono::Utc::now() + chrono::Duration::days(2);
    let ev = uid(&admin
        .insert(
            "CalendarEvent",
            json!({"subject": "Spares review", "starts_at": soon.to_rfc3339(), "ends_at": (soon + chrono::Duration::minutes(45)).to_rfc3339(), "owner": "anna@tier1.test",
                   "attendees": [{"email": "Jane@Acme.test"}, {"email": "anna@tier1.test"}, {"email": "stranger@x.test"}]}),
        )
        .await);
    drain(k).await;
    let meeting = admin.one("Activity", json!({"calendar_event": ev.to_string()})).await;
    assert_eq!(
        (meeting["activity_type"].clone(), meeting["contact"].clone(), meeting["opportunity"].clone()),
        (json!("Meeting"), json!(jane.to_string()), json!(d1.to_string()))
    );
    assert_eq!(
        (meeting["assigned_to"].clone(), meeting["status"].clone(), meeting["duration_minutes"].clone()),
        (json!("anna@tier1.test"), json!("Open"), json!(45))
    );
    assert_eq!(titles(&anna.timeline("Contact", jane, json!({"kinds": "meeting"})).await, "meeting"), ["Spares review"]);
    let moved = soon + chrono::Duration::days(1);
    admin
        .update(
            "CalendarEvent",
            ev,
            json!({"starts_at": moved.to_rfc3339(), "ends_at": (moved + chrono::Duration::minutes(45)).to_rfc3339(), "subject": "Spares review (moved)"}),
        )
        .await;
    drain(k).await;
    let meeting = admin.one("Activity", json!({"calendar_event": ev.to_string()})).await;
    assert_eq!(meeting["subject"], json!("Spares review (moved)"));
    assert_eq!(chrono::DateTime::parse_from_rfc3339(meeting["due_at"].as_str().unwrap()).unwrap().timestamp(), moved.timestamp(), "it moved with the event");
    assert_eq!(admin.list("Activity", json!({"calendar_event": ev.to_string()})).await.len(), 1, "the same activity");
    admin.update("CalendarEvent", ev, json!({"status": "Cancelled"})).await;
    drain(k).await;
    assert_eq!(admin.one("Activity", json!({"calendar_event": ev.to_string()})).await["status"], json!("Cancelled"));
    admin.update("CalendarEvent", ev, json!({"status": "Confirmed"})).await;
    drain(k).await;
    assert_eq!(admin.one("Activity", json!({"calendar_event": ev.to_string()})).await["status"], json!("Open"), "confirmed again");
    admin.k.delete(&admin.ctx, "CalendarEvent", ev, None).await.unwrap();
    drain(k).await;
    assert_eq!(admin.one("Activity", json!({"calendar_event": ev.to_string()})).await["status"], json!("Cancelled"), "a deleted event cancels it");
    // a meeting that has happened is done, and touches its lead
    let past = chrono::Utc::now() - chrono::Duration::hours(3);
    let ev2 = uid(&admin
        .insert(
            "CalendarEvent",
            json!({"subject": "Pat intro", "starts_at": past.to_rfc3339(), "ends_at": (past + chrono::Duration::hours(1)).to_rfc3339(), "owner": "anna@tier1.test", "attendees": [{"email": "pat@lead.test"}]}),
        )
        .await);
    drain(k).await;
    ferrum_app_crm::maintenance(k).await.unwrap();
    assert_eq!(admin.one("Activity", json!({"calendar_event": ev2.to_string()})).await["status"], json!("Done"));
    assert_eq!(anna.get("Lead", pat).await["status"], json!("Contacted"));
    // nobody known on the guest list: no activity
    admin
        .insert("CalendarEvent", json!({"subject": "Strangers", "starts_at": soon.to_rfc3339(), "ends_at": (soon + chrono::Duration::hours(1)).to_rfc3339(), "attendees": [{"email": "stranger@x.test"}]}))
        .await;
    drain(k).await;
    assert!(admin.list("Activity", json!({"subject": "Strangers"})).await.is_empty());
    // the setting off: nothing is filed
    set(json!({"file_communications": false})).await;
    mail(admin, "Received", "jane@acme.test", "anna@tier1.test", "While filing is off", "Email").await;
    drain(k).await;
    assert!(!titles(&anna.timeline("Contact", jane, json!({})).await, "email").contains(&"While filing is off".to_string()));
    set(json!({"file_communications": true})).await;
}

/// What Mail raises for a message of a hosted mailbox whose owner allowed filing (`MailMessage.received`, `.sent`).
async fn raise(k: &Kernel, tenant: Uuid, kind: &str, record: Uuid, from: &str, to: &[&str], subject: &str) {
    let mut tx = k.db.system_tenant_tx(tenant).await.unwrap();
    let payload = json!({"kind": kind, "mailbox": Uuid::nil(), "mailbox_address": "anna@tier1.test", "owner": null, "account": "a1", "email": "e1", "thread": "T1",
                         "message_id": "m1@x.test", "in_reply_to": [], "from": from, "to": to, "cc": [], "subject": subject, "at": chrono::Utc::now().to_rfc3339()});
    ferrum_kernel::outbox::enqueue(&mut tx, tenant, &format!("MailMessage.{kind}"), Some("MailMessage"), Some(record), payload).await.unwrap();
    tx.commit().await.unwrap();
}

async fn filed_mail(k: &Kernel, tenant: Uuid, message: Uuid) -> i64 {
    scalar(k, "select count(*) from crm_filed_mail where tenant_id = $1 and message = $2", tenant, Some(message)).await
}

/// Hosted mailboxes file by themselves: by address, on the same people and the newest open deal as a Communication,
/// under the same settings, once for each message however often and however concurrently it is told, and an
/// erasure reaches what is filed.
async fn hosted_mail(k: &Arc<Kernel>, admin: &U, anna: &U, tenant: Uuid) {
    let settings = admin.one("CrmSettings", json!({})).await;
    let lee = uid(&anna.insert("Lead", json!({"first_name": "Lee", "last_name": "Hosted", "email": "Lee@Hosted.test", "source": "Website"})).await);
    let deal = uid(&anna.insert("Opportunity", json!({"opportunity_from": "Lead", "lead": lee.to_string(), "title": "Lee's deal", "amount": "10"})).await);
    let m1 = Uuid::now_v7();
    raise(k, tenant, "received", m1, "lee@hosted.test", &["anna@tier1.test"], "Hosted hello").await;
    // The same message told again, and at once from two workers: still one filing (a lead and a deal)
    raise(k, tenant, "received", m1, "lee@hosted.test", &["anna@tier1.test"], "Hosted hello").await;
    tokio::join!(drain(k), drain(k));
    assert_eq!(filed_mail(k, tenant, m1).await, 2, "on the lead and on its deal, once");
    let t = anna.timeline("Lead", lee, json!({})).await;
    assert_eq!(titles(&t, "email"), ["Hosted hello"]);
    assert_eq!(t["events"][0]["details"]["sender"], json!("lee@hosted.test"));
    // what opens it in Mail: its hosted mailbox and its id there (from the event, never an address)
    assert_eq!((t["events"][0]["details"]["mailbox"].clone(), t["events"][0]["details"]["email"].clone()), (json!(Uuid::nil().to_string()), json!("e1")));
    assert_eq!(titles(&anna.timeline("Opportunity", deal, json!({})).await, "email"), ["Hosted hello"]);
    assert!(anna.get("Lead", lee).await["last_touched_at"].is_string(), "heard from: touched");
    // one that anna sent him is filed too; one between colleagues, to nobody known, or to outsiders only, is not
    let m2 = Uuid::now_v7();
    raise(k, tenant, "sent", m2, "anna@tier1.test", &["lee@hosted.test", "ben@tier1.test"], "Re: Hosted hello").await;
    let (m3, m4) = (Uuid::now_v7(), Uuid::now_v7());
    raise(k, tenant, "sent", m3, "anna@tier1.test", &["ben@tier1.test"], "Internal").await;
    raise(k, tenant, "received", m4, "stranger@hosted-new.test", &["anna@tier1.test"], "Who is this").await;
    drain(k).await;
    assert_eq!(titles(&anna.timeline("Lead", lee, json!({})).await, "email"), ["Re: Hosted hello", "Hosted hello"]);
    assert_eq!((filed_mail(k, tenant, m3).await, filed_mail(k, tenant, m4).await), (0, 0));
    assert!(admin.list("Lead", json!({"email": "stranger@hosted-new.test"})).await.is_empty(), "no lead unless the setting says so");
    // the unknown-sender setting makes a lead of a stranger, never of an automatic sender
    admin.update("CrmSettings", uid(&settings), json!({"unknown_sender_lead": true})).await;
    let (m5, m6) = (Uuid::now_v7(), Uuid::now_v7());
    raise(k, tenant, "received", m5, "stranger@hosted-new.test", &["anna@tier1.test"], "Who is this").await;
    raise(k, tenant, "received", m6, "no-reply@hosted-news.test", &["anna@tier1.test"], "Newsletter").await;
    drain(k).await;
    let stranger = admin.one("Lead", json!({"email": "stranger@hosted-new.test"})).await;
    assert_eq!(titles(&admin.timeline("Lead", uid(&stranger), json!({})).await, "email"), ["Who is this"]);
    assert!(admin.list("Lead", json!({"email": "no-reply@hosted-news.test"})).await.is_empty());
    admin.update("CrmSettings", uid(&settings), json!({"unknown_sender_lead": false})).await;
    // the setting that files is CRM's too: off, nothing is filed
    admin.update("CrmSettings", uid(&settings), json!({"file_communications": false})).await;
    let m7 = Uuid::now_v7();
    raise(k, tenant, "received", m7, "lee@hosted.test", &["anna@tier1.test"], "While it was off").await;
    drain(k).await;
    assert_eq!(filed_mail(k, tenant, m7).await, 0);
    admin.update("CrmSettings", uid(&settings), json!({"file_communications": true})).await;
    // a reply stops the sequence the person was in (as for a Communication)
    let seq = uid(&admin
        .insert(
            "Sequence",
            json!({"sequence_name": "Hosted follow-up", "applies_to": "Lead", "steps": [{"step_type": "Task", "after_days": 5, "subject": "Call"}]}),
        )
        .await);
    anna.action("Lead", lee, "enrol_in_sequence", json!({"sequence": seq.to_string()})).await;
    let m8 = Uuid::now_v7();
    raise(k, tenant, "received", m8, "lee@hosted.test", &["anna@tier1.test"], "Interested").await;
    drain(k).await;
    assert_eq!(admin.one("SequenceEnrolment", json!({"lead": lee.to_string()})).await["stopped_reason"], json!("Replied"));

    // ── erasure reaches what is filed: reported, then gone, the record's other mail untouched
    let found = k.personal_data(&admin.ctx, "lee@hosted.test").await.unwrap();
    assert!(found.to_string().contains("MailMessage"), "{found}");
    k.erase_personal_data(&admin.ctx, "lee@hosted.test", "Request 12").await.unwrap();
    assert_eq!(filed_mail(k, tenant, m1).await + filed_mail(k, tenant, m2).await + filed_mail(k, tenant, m8).await, 0);
    let left: i64 =
        scalar(k, "select count(*) from crm_filed_mail where tenant_id = $1 and subject is not null and record_id <> $2", tenant, Some(Uuid::nil())).await;
    assert_eq!(left, 1, "only the stranger's, on the lead made of it: {left}");
}

async fn run_steps(k: &Kernel) {
    ferrum_app_crm::maintenance(k).await.expect("maintenance");
}

async fn due_now(k: &Kernel, tenant: Uuid) {
    let mut tx = k.db.system_tenant_tx(tenant).await.unwrap();
    sqlx::query("update e_sequence_enrolment set next_run_at = now() - interval '1 minute' where tenant_id = $1 and status = 'Active'")
        .bind(tenant)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
}

async fn sequences(k: &Arc<Kernel>, admin: &U, anna: &U, tenant: Uuid) {
    // A message template the workspace wrote, and a business calendar the waits count on.
    let template: ferrum_meta::MessageDef = serde_json::from_value(json!({
        "key": "x_seq_hello", "entity": "Lead",
        "languages": {"en": {"email": {"subject": "Hello {{ doc.lead_name }}", "body": "<p>Hello {{ doc.lead_name }}</p>"}}}
    }))
    .unwrap();
    k.save_message_template(&admin.ctx, &template).await.unwrap();
    k.put_logic_calendar(&admin.ctx, "sales_days", &json!({"zone": "UTC", "weekends": ["sat", "sun"], "holidays": []})).await.unwrap();
    let seq = uid(&admin
        .insert(
            "Sequence",
            json!({"sequence_name": "Welcome", "applies_to": "Lead", "calendar": "sales_days",
                   "steps": [{"step_type": "Task", "after_days": 0, "subject": "Prepare the intro call"},
                             {"step_type": "Message", "after_days": 2, "message": "x_seq_hello"},
                             {"step_type": "Call", "after_days": 1, "subject": "Follow-up call"}]}),
        )
        .await)
    .to_string();
    refused(admin.k.insert(&admin.ctx, "Sequence", obj(json!({"sequence_name": "Empty", "applies_to": "Lead"}))).await, "Add at least one step");
    let lead = |name: &'static str| async move {
        uid(&anna.insert("Lead", json!({"first_name": name, "last_name": "Seq", "email": format!("{}@seq.test", name.to_lowercase())})).await)
    };
    let (amy, bo, cy, di, ed) = (lead("Amy").await, lead("Bo").await, lead("Cy").await, lead("Di").await, lead("Ed").await);

    // ── enrolling: once for each person, by the action or by anything that creates an enrolment
    let made = anna.action("Lead", amy, "enrol_in_sequence", json!({"sequence": seq})).await;
    let en = vid(&made);
    let e = anna.get("SequenceEnrolment", en).await;
    assert_eq!((e["status"].clone(), e["next_step"].clone()), (json!("Active"), json!(1)));
    assert_eq!(e["title"], json!("Amy Seq: Welcome"));
    refused(anna.k.run_action(&anna.ctx, "Lead", amy, "enrol_in_sequence", json!({"sequence": seq})).await, "once");
    let by_automation = anna.insert("SequenceEnrolment", json!({"sequence": seq, "lead": bo.to_string()})).await;
    refused(anna.k.insert(&anna.ctx, "SequenceEnrolment", obj(json!({"sequence": seq, "lead": bo.to_string()}))).await, "already has this Sequence");
    let contact_seq = anna.k.insert(&anna.ctx, "SequenceEnrolment", obj(json!({"sequence": seq, "contact": Uuid::nil().to_string()}))).await;
    assert!(contact_seq.is_err(), "a lead's sequence is not a contact's: {contact_seq:?}");
    let enrolled = |id: Uuid| async move { anna.get("SequenceEnrolment", id).await };

    // ── the first step is due at once: a task for the lead's owner; the next waits two working days
    run_steps(k).await;
    let task = anna.one("Activity", json!({"subject": "Prepare the intro call", "lead": amy.to_string()})).await;
    assert_eq!((task["activity_type"].clone(), task["assigned_to"].clone(), task["status"].clone()), (json!("Task"), json!("anna@tier1.test"), json!("Open")));
    let e = enrolled(en).await;
    assert_eq!(e["next_step"], json!(2));
    let due = chrono::DateTime::parse_from_rfc3339(e["next_run_at"].as_str().unwrap()).unwrap();
    let wait = due.signed_duration_since(chrono::Utc::now());
    assert!(wait > chrono::Duration::hours(47) && wait < chrono::Duration::days(5), "two working days on a calendar with weekends: {wait}");
    run_steps(k).await;
    assert_eq!(anna.list("Activity", json!({"lead": amy.to_string()})).await.len(), 1, "nothing runs before it is due: one step once");

    // ── the message step sends the template and puts the mail on the lead
    due_now(k, tenant).await;
    run_steps(k).await;
    let sent = anna.k.message_log(&anna.ctx, Some(("Lead", amy)), None, 10).await.unwrap();
    assert!(sent.iter().any(|m| m["template"] == "x_seq_hello" && m["recipient"] == "amy@seq.test"), "{sent:?}");
    assert_eq!(anna.list("Activity", json!({"lead": amy.to_string(), "activity_type": "Email", "status": "Done"})).await.len(), 1);
    assert_eq!(enrolled(en).await["next_step"], json!(3));

    // ── a reply ends it: the Call step never runs
    mail(admin, "Received", "amy@seq.test", "anna@tier1.test", "Re: Hello Amy", "Email").await;
    drain(k).await;
    let e = enrolled(en).await;
    assert_eq!((e["status"].clone(), e["stopped_reason"].clone()), (json!("Stopped"), json!("Replied")));
    due_now(k, tenant).await;
    run_steps(k).await;
    assert!(anna.list("Activity", json!({"lead": amy.to_string(), "activity_type": "Call"})).await.is_empty());

    // ── booking a meeting, opting out, conversion and the owner's own stop, each with its reason
    let by_mail = |id: Uuid| async move { anna.get("SequenceEnrolment", id).await };
    let cy_en = vid(&anna.action("Lead", cy, "enrol_in_sequence", json!({"sequence": seq})).await);
    let soon = chrono::Utc::now() + chrono::Duration::days(3);
    admin
        .insert("CalendarEvent", json!({"subject": "Cy demo", "starts_at": soon.to_rfc3339(), "ends_at": (soon + chrono::Duration::hours(1)).to_rfc3339(), "owner": "anna@tier1.test", "attendees": [{"email": "cy@seq.test"}]}))
        .await;
    drain(k).await;
    assert_eq!(by_mail(cy_en).await["stopped_reason"], json!("Booked a Meeting"));
    let di_en = vid(&anna.action("Lead", di, "enrol_in_sequence", json!({"sequence": seq})).await);
    anna.update("Lead", di, json!({"unsubscribed": true})).await;
    assert_eq!(by_mail(di_en).await["stopped_reason"], json!("Opted Out"));
    refused(anna.k.insert(&anna.ctx, "SequenceEnrolment", obj(json!({"sequence": seq, "lead": di.to_string()}))).await, "asked not to be mailed");
    assert_eq!(by_mail(uid(&by_automation)).await["status"], json!("Completed"), "the one an automation made ran all its steps");
    let hal = lead("Hal").await;
    let bo_en = vid(&anna.action("Lead", hal, "enrol_in_sequence", json!({"sequence": seq})).await);
    anna.action("SequenceEnrolment", bo_en, "stop_enrolment", json!({})).await;
    assert_eq!(by_mail(bo_en).await["stopped_reason"], json!("Stopped by Owner"));
    let ed_en = vid(&anna.action("Lead", ed, "enrol_in_sequence", json!({"sequence": seq})).await);
    anna.action("Lead", ed, "convert_to_customer", json!({})).await;
    due_now(k, tenant).await;
    run_steps(k).await;
    assert_eq!(by_mail(ed_en).await["stopped_reason"], json!("Converted"));
    assert!(anna.list("Activity", json!({"lead": ed.to_string()})).await.is_empty(), "no step ran for a converted lead");

    // ── a sequence switched off runs nothing; one that does not stop on replies does not
    let seq_id: Uuid = seq.parse().unwrap();
    let quiet = uid(&admin.insert("Sequence", json!({"sequence_name": "Keep going", "applies_to": "Lead", "stop_on_reply": false, "steps": [{"step_type": "Task", "after_days": 0, "subject": "Ring"}]})).await);
    let fay = lead("Fay").await;
    let fay_en = vid(&anna.action("Lead", fay, "enrol_in_sequence", json!({"sequence": quiet.to_string()})).await);
    mail(admin, "Received", "fay@seq.test", "anna@tier1.test", "Re: nothing", "Email").await;
    drain(k).await;
    assert_eq!(by_mail(fay_en).await["status"], json!("Active"), "this sequence does not stop on a reply");
    admin.update("Sequence", quiet, json!({"enabled": false})).await;
    run_steps(k).await;
    assert!(anna.list("Activity", json!({"subject": "Ring"})).await.is_empty(), "switched off: nothing runs");
    admin.update("Sequence", quiet, json!({"enabled": true})).await;
    run_steps(k).await;
    assert_eq!(anna.list("Activity", json!({"subject": "Ring"})).await.len(), 1);
    assert_eq!(by_mail(fay_en).await["status"], json!("Completed"), "the last step done");
    let _ = seq_id;

    // ── two replicas at once: one step, once
    let gus = lead("Gus").await;
    anna.action("Lead", gus, "enrol_in_sequence", json!({"sequence": quiet.to_string()})).await;
    let _ = tokio::join!(ferrum_app_crm::maintenance(k), ferrum_app_crm::maintenance(k));
    assert_eq!(anna.list("Activity", json!({"lead": gus.to_string(), "subject": "Ring"})).await.len(), 1);
}

async fn forecast(k: &Arc<Kernel>, admin: &U, anna: &U, ben: &U, maria: &U, tenant: Uuid) {
    // The upkeep of the earlier sections took snapshots of Anna's other deals: this section starts from none.
    {
        let mut tx = k.db.system_tenant_tx(tenant).await.unwrap();
        sqlx::query("delete from e_forecast_snapshot where tenant_id = $1").bind(tenant).execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
    }
    let stages = admin.list("SalesStage", json!({})).await;
    let stage = |name: &str| stages.iter().find(|s| s["stage_name"] == json!(name) && s["pipeline"] == stages[0]["pipeline"]).map(|s| s["id"].clone()).unwrap();
    let acme = uid(&admin.one("Customer", json!({"customer_name": "Acme Corp"})).await);
    let deal = |amount: &'static str, st: &'static str| async move {
        uid(&anna.insert("Opportunity", json!({"opportunity_from": "Customer", "customer": acme.to_string(), "amount": amount, "expected_close_date": today().to_string(), "sales_stage": stage(st)})).await)
    };
    deal("1000", "Negotiation").await;
    deal("500", "Proposal").await;
    deal("300", "Qualification").await;
    let won = deal("700", "Qualification").await;
    anna.action("Opportunity", won, "mark_won", json!({})).await;

    // ── the forecast: a line for each category, from the deals, the owner's call empty
    let f = uid(&anna.insert("ForecastSubmission", json!({"period": "Month", "start_date": today().to_string()})).await);
    let doc = anna.get("ForecastSubmission", f).await;
    assert_eq!(doc["assigned_to"], json!("anna@tier1.test"));
    let by = |d: &Doc, c: &str| d["lines"].as_array().unwrap().iter().find(|l| l["category"] == json!(c)).cloned().unwrap();
    assert_eq!(doc["lines"].as_array().unwrap().iter().map(|l| l["category"].as_str().unwrap()).collect::<Vec<_>>(), ["Commit", "Best Case", "Pipeline"]);
    assert_eq!(
        (dec(&by(&doc, "Commit")["calculated"]), dec(&by(&doc, "Best Case")["calculated"]), dec(&by(&doc, "Pipeline")["calculated"])),
        (n("1000"), n("500"), n("300"))
    );
    assert_eq!(dec(&doc["closed"]), n("700"));
    assert_eq!(doc["status"], json!("Open"));
    refused(
        anna.k.insert(&anna.ctx, "ForecastSubmission", obj(json!({"period": "Month", "start_date": today().to_string()}))).await,
        "already has this Forecast Owner",
    );
    refused(ben.k.get(&ben.ctx, "ForecastSubmission", f, false).await, "not found");

    // ── the owner calls Commit lower than the deals say, and submits: a call left empty is the deals' own
    let commit = by(&doc, "Commit");
    let doc = anna.update("ForecastSubmission", f, json!({"lines": [{"id": commit["id"], "category": "Commit", "amount": "900", "adjusted": "99999"}]})).await;
    assert_eq!(dec(&by(&doc, "Commit")["amount"]), n("900"));
    assert!(by(&doc, "Commit")["adjusted"].is_null(), "the manager's call is not the owner's to write");
    refused(anna.k.run_action(&anna.ctx, "ForecastSubmission", f, "adjust_forecast", json!({"category": "Commit", "amount": "1"})).await, "not available");
    anna.action("ForecastSubmission", f, "submit_forecast", json!({})).await;
    let doc = anna.get("ForecastSubmission", f).await;
    assert_eq!(doc["status"], json!("Submitted"));
    assert_eq!((dec(&by(&doc, "Commit")["amount"]), dec(&by(&doc, "Best Case")["amount"])), (n("900"), n("500")));
    refused(
        anna.k
            .update(&anna.ctx, "ForecastSubmission", f, obj(json!({"lines": [{"id": by(&doc, "Commit")["id"], "category": "Commit", "amount": "950"}]})), None)
            .await,
        "reopen",
    );

    // ── the manager sets another figure beside it: both are kept; the owner cannot, and nor can someone else's manager
    refused(anna.k.run_action(&anna.ctx, "ForecastSubmission", f, "adjust_forecast", json!({"category": "Commit", "amount": "1200"})).await, "adjusts");
    refused(maria.k.run_action(&maria.ctx, "ForecastSubmission", f, "adjust_forecast", json!({"category": "Gossip", "amount": "1"})).await, "category");
    maria.action("ForecastSubmission", f, "adjust_forecast", json!({"category": "Commit", "amount": "1100", "note": "The Acme order is firm"})).await;
    let doc = anna.get("ForecastSubmission", f).await;
    let c = by(&doc, "Commit");
    assert_eq!(
        (dec(&c["amount"]), dec(&c["adjusted"]), c["adjusted_by"].clone(), c["note"].clone()),
        (n("900"), n("1100"), json!("maria@tier1.test"), json!("The Acme order is firm"))
    );

    // ── snapshots on a schedule: one for the owner today, not another until the days are up
    run_steps(k).await;
    let snaps = admin.list("ForecastSnapshot", json!({"assigned_to": "anna@tier1.test"})).await;
    assert_eq!(snaps.len(), 1);
    assert_eq!(dec(&snaps[0]["closed"]), n("700"));
    assert_eq!(
        (dec(&snaps[0]["figures"]["Commit"]["calculated"]), dec(&snaps[0]["figures"]["Commit"]["submitted"]), dec(&snaps[0]["figures"]["Commit"]["adjusted"])),
        (n("1000"), n("900"), n("1100"))
    );
    run_steps(k).await;
    assert_eq!(admin.list("ForecastSnapshot", json!({"assigned_to": "anna@tier1.test"})).await.len(), 1, "once for the days between");
    assert_eq!(maria.list("ForecastSnapshot", json!({"assigned_to": "anna@tier1.test"})).await.len(), 1, "her manager sees it");
    assert!(ben.list("ForecastSnapshot", json!({"assigned_to": "anna@tier1.test"})).await.is_empty());
    // a week on: the deals moved, and the next snapshot shows by how much
    {
        let mut tx = k.db.system_tenant_tx(tenant).await.unwrap();
        sqlx::query("update e_forecast_snapshot set taken_on = taken_on - 7 where tenant_id = $1").bind(tenant).execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
    }
    let bigger = uid(&admin.one("Opportunity", json!({"amount": "1000"})).await);
    anna.update("Opportunity", bigger, json!({"amount": "1500", "items": []})).await;
    run_steps(k).await;
    assert_eq!(admin.list("ForecastSnapshot", json!({"assigned_to": "anna@tier1.test"})).await.len(), 2);
    let params = [("from_date", (today() - chrono::Duration::days(30)).to_string()), ("to_date", (today() + chrono::Duration::days(30)).to_string())];
    let report = |name: &'static str, who: &U| {
        let p: std::collections::HashMap<String, String> = params.iter().map(|(a, b)| (a.to_string(), b.clone())).collect();
        let (k, ctx) = (who.k.clone(), who.ctx.clone());
        async move { ferrum_domain::reports::run(&k, &ctx, name, &p).await.unwrap_or_else(|e| panic!("{name}: {e:?}")) }
    };
    let h = report("forecast_history", maria).await;
    let rows = h["rows"].as_array().unwrap();
    let mine: Vec<&Value> = rows.iter().filter(|r| r["owner"] == json!("anna@tier1.test")).collect();
    assert_eq!(mine.len(), 2);
    assert_eq!(mine[0]["commit_moved"], Value::Null, "the first has nothing before it");
    assert_eq!(dec(&mine[1]["commit_moved"]), n("0"), "the manager's 1100 still stands over the deals' change");
    assert_eq!(dec(&mine[0]["commit"]), n("1100"));
    // ── forecast against closed: what was called, by the owner and the manager, and what was won
    let r = report("forecast_vs_closed", maria).await;
    let a = r["rows"].as_array().unwrap().iter().find(|r| r["owner"] == json!("anna@tier1.test")).unwrap().clone();
    assert_eq!((dec(&a["commit"]), dec(&a["commit_manager"]), dec(&a["closed"])), (n("900"), n("1100"), n("700")));
    assert_eq!(dec(&a["closed_of_commit_pct"]), n("63.64"));
    assert_eq!(dec(&a["gap_to_commit"]), n("-400"));
    assert!(report("forecast_vs_closed", ben).await["rows"].as_array().unwrap().is_empty(), "only forecasts the caller may read");

    // ── reopening clears the manager's call, keeps the owner's, and lets the owner change it
    maria.action("ForecastSubmission", f, "reopen_forecast", json!({})).await;
    let doc = anna.get("ForecastSubmission", f).await;
    assert_eq!((doc["status"].clone(), by(&doc, "Commit")["adjusted"].clone(), dec(&by(&doc, "Commit")["amount"])), (json!("Open"), Value::Null, n("900")));
    // two upkeep runs at once take one snapshot
    {
        let mut tx = k.db.system_tenant_tx(tenant).await.unwrap();
        sqlx::query("delete from e_forecast_snapshot where tenant_id = $1").bind(tenant).execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
    }
    let _ = tokio::join!(ferrum_app_crm::maintenance(k), ferrum_app_crm::maintenance(k));
    assert_eq!(admin.list("ForecastSnapshot", json!({"assigned_to": "anna@tier1.test"})).await.len(), 1);
    // the fiscal year starts in April: a quarter is Oct to Dec
    let q = uid(&anna.insert("ForecastSubmission", json!({"period": "Quarter", "start_date": "2026-11-20"})).await);
    let q = anna.get("ForecastSubmission", q).await;
    assert_eq!((q["start_date"].clone(), q["end_date"].clone()), (json!("2026-10-01"), json!("2026-12-31")));
}

async fn record_types(admin: &U, anna: &U) {
    let pipeline = uid(&admin.insert("SalesPipeline", json!({"pipeline_name": "Enterprise"})).await).to_string();
    for (i, (name, p)) in [("Discovery", 10), ("Proposal", 50), ("Contract", 80)].iter().enumerate() {
        admin.insert("SalesStage", json!({"pipeline": pipeline, "stage_name": name, "probability": p, "stage_order": i + 1})).await;
    }
    refused(
        admin
            .k
            .insert(
                &admin.ctx,
                "RecordType",
                obj(json!({"type_name": "Bad", "applies_to": "Opportunity", "defaults": [{"field": "no_such_field", "value": "x"}]})),
            )
            .await,
        "no_such_field",
    );
    let kind = uid(&admin
        .insert(
            "RecordType",
            json!({"type_name": "Enterprise deal", "applies_to": "Opportunity", "pipeline": pipeline,
                   "defaults": [{"field": "source", "value": "Partner"}],
                   "requirements": [{"stage_name": "Proposal", "field": "next_step"}, {"stage_name": "Contract", "field": "expected_close_date"}]}),
        )
        .await)
    .to_string();
    let acme = uid(&admin.one("Customer", json!({"customer_name": "Acme Corp"})).await).to_string();
    // a deal of the type goes in its pipeline at the first open stage, with its defaults, and asks for what it says before a stage
    let d = anna.insert("Opportunity", json!({"opportunity_from": "Customer", "customer": acme, "record_type": kind, "amount": "100"})).await;
    assert_eq!(d["pipeline"], json!(pipeline));
    assert_eq!(d["source"], json!("Partner"));
    let stage = admin.one("SalesStage", json!({"pipeline": pipeline, "stage_name": "Proposal"})).await;
    refused(anna.k.update(&anna.ctx, "Opportunity", uid(&d), obj(json!({"sales_stage": stage["id"]})), None).await, "asks for Next Step before Proposal");
    anna.update("Opportunity", uid(&d), json!({"sales_stage": stage["id"], "next_step": "Send the proposal"})).await;
    // a deal of no type is as before; a type of another entity, or switched off, is refused
    let plain = anna.insert("Opportunity", json!({"opportunity_from": "Customer", "customer": acme, "amount": "100"})).await;
    assert_ne!(plain["pipeline"], json!(pipeline));
    assert_eq!(plain["source"], Value::Null);
    let lead_kind = uid(&admin
        .insert("RecordType", json!({"type_name": "Event lead", "applies_to": "Lead", "defaults": [{"field": "source", "value": "Trade Show"}], "requirements": [{"stage_name": "Qualified", "field": "budget"}]}))
        .await)
    .to_string();
    refused(
        anna.k.insert(&anna.ctx, "Opportunity", obj(json!({"opportunity_from": "Customer", "customer": acme, "record_type": lead_kind}))).await,
        "record_type",
    );
    let l = anna.insert("Lead", json!({"first_name": "Kit", "last_name": "Typed", "record_type": lead_kind})).await;
    assert_eq!(l["source"], json!("Trade Show"));
    refused(anna.k.update(&anna.ctx, "Lead", uid(&l), obj(json!({"status": "Qualified"})), None).await, "asks for Budget before Qualified");
    anna.update("Lead", uid(&l), json!({"status": "Qualified", "budget": "5000"})).await;
    // allowed stages: a deal of the type moves through those only (none listed: all, as before); winning is never held up
    let pipeline_id = pipeline.clone();
    let stage_of = move |name: &'static str| {
        let pipeline = pipeline_id.clone();
        async move { admin.one("SalesStage", json!({"pipeline": pipeline, "stage_name": name})).await["id"].clone() }
    };
    let stage_of = &stage_of;
    let staged = uid(&admin
        .insert(
            "RecordType",
            json!({"type_name": "Fast deal", "applies_to": "Opportunity", "pipeline": pipeline,
                   "allowed_stages": [{"stage": stage_of("Discovery").await}, {"stage": stage_of("Contract").await}]}),
        )
        .await)
    .to_string();
    let f = anna.insert("Opportunity", json!({"opportunity_from": "Customer", "customer": acme, "record_type": staged, "amount": "100"})).await;
    refused(
        anna.k.update(&anna.ctx, "Opportunity", uid(&f), obj(json!({"sales_stage": stage_of("Proposal").await})), None).await,
        "Fast deal does not take a deal to Proposal: its deals move through Discovery, Contract",
    );
    anna.update("Opportunity", uid(&f), json!({"sales_stage": stage_of("Contract").await})).await;
    anna.update("Opportunity", uid(&f), json!({"status": "Won"})).await;
    let other = anna.insert("Opportunity", json!({"opportunity_from": "Customer", "customer": acme, "amount": "100"})).await;
    anna.update("Opportunity", uid(&other), json!({"sales_stage": stage_of("Proposal").await})).await;
    admin.update("RecordType", vid(&json!({"id": lead_kind})), json!({"disabled": true})).await;
    refused(anna.k.insert(&anna.ctx, "Lead", obj(json!({"first_name": "Off", "record_type": lead_kind}))).await, "switched off");
    prospect_activities(admin, anna).await;
}

/// An activity is linked to a prospect, directly or through its lead, contact or deal, and shows on the prospect's timeline.
async fn prospect_activities(admin: &U, anna: &U) {
    let p = uid(&admin.insert("Prospect", json!({"prospect_name": "Prospect Quick Actions Ltd"})).await);
    let q = uid(&admin.insert("Prospect", json!({"prospect_name": "Other Prospect Ltd"})).await);
    let lead = uid(&anna.insert("Lead", json!({"first_name": "Pia", "last_name": "Prospect", "prospect": p.to_string()})).await);
    let through = anna.insert("Activity", json!({"activity_type": "Note", "subject": "Through the lead", "lead": lead.to_string()})).await;
    assert_eq!(through["prospect"], json!(p.to_string()), "taken from its lead");
    let direct = anna.insert("Activity", json!({"activity_type": "Task", "subject": "Straight on the prospect", "prospect": p.to_string()})).await;
    assert_eq!(direct["prospect"], json!(p.to_string()));
    anna.insert("Activity", json!({"activity_type": "Note", "subject": "Elsewhere", "prospect": q.to_string()})).await;
    let t = anna.timeline("Prospect", p, json!({})).await;
    let mut seen: Vec<String> = t["events"].as_array().unwrap().iter().filter_map(|e| e["title"].as_str().map(str::to_string)).collect();
    seen.sort();
    assert_eq!(seen, ["Straight on the prospect", "Through the lead"], "the prospect's own timeline");
}
