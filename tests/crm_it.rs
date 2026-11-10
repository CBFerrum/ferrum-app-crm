//! End-to-end CRM test against a real PostgreSQL (`FERRUM_CRM_TEST_DATABASE_URL`, default
//! `postgres://ferrum:ferrum@localhost:5432/ferrum_crm_test`). The database is reset first.
//!
//! Two sales reps and their manager work leads from a trade-show campaign through activities,
//! opportunities and quotations to sales orders, with the refusal paths along the way: duplicate
//! emails, converting twice, ordering a lead's or an expired quotation, losing without a reason,
//! out-of-range probabilities, over-ordering, and what a sales user may not see or do.

use ferrum_kernel::tenant::NewTenant;
use ferrum_kernel::{Ctx, Db, DbConfig, Doc, Error, HookRegistry, Kernel, KernelSettings, ListQuery};
use ferrum_meta::Catalog;
use rust_decimal::Decimal;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use uuid::Uuid;

const PASSWORD: &str = "Correct-Horse-9";

fn obj(v: Value) -> Doc {
    v.as_object().cloned().unwrap()
}

fn dec(v: &Value) -> Decimal {
    match v {
        Value::String(s) => Decimal::from_str(s).unwrap_or_else(|_| panic!("not a decimal: {s}")),
        Value::Number(n) => Decimal::from_str(&n.to_string()).unwrap(),
        Value::Null => Decimal::ZERO,
        other => panic!("not a decimal: {other}"),
    }
}

fn n(s: &str) -> Decimal {
    Decimal::from_str(s).unwrap()
}

fn uid(doc: &Doc) -> Uuid {
    doc["id"].as_str().unwrap().parse().unwrap()
}

fn vid(v: &Value) -> Uuid {
    v["id"].as_str().unwrap().parse().unwrap()
}

/// The workspace's zone: one whose date is not UTC's while the suite runs (UTC+14 from 11:00 UTC,
/// UTC-12 before), at least an hour from its midnight, so every date the app stamps must be the
/// workspace's to pass.
static ZONE: std::sync::LazyLock<&str> =
    std::sync::LazyLock::new(|| if chrono::Timelike::hour(&chrono::Utc::now()) >= 11 { "Pacific/Kiritimati" } else { "Etc/GMT+12" });

fn today() -> chrono::NaiveDate {
    ferrum_kernel::ctx::local_now(&ZONE).date_naive()
}

/// A refusal of the expected kind whose text mentions `needle` (field errors: field or message).
fn refused<T: std::fmt::Debug>(r: Result<T, Error>, needle: &str) {
    match &r {
        Err(Error::Invalid(m)) | Err(Error::Forbidden(m)) if m.contains(needle) => {}
        Err(Error::Validation(errs)) if errs.iter().any(|e| e.field == needle || e.message.contains(needle)) => {}
        Err(Error::NotFound(_)) if needle == "not found" => {}
        other => panic!("expected a refusal mentioning '{needle}', got {other:?}"),
    }
}

async fn fresh_kernel() -> Arc<Kernel> {
    let url = std::env::var("FERRUM_CRM_TEST_DATABASE_URL").unwrap_or_else(|_| "postgres://ferrum:ferrum@localhost:5432/ferrum_crm_test".into());
    let db = Db::connect(DbConfig { url, max_connections: 10, ..Default::default() }).await.expect("connect");
    sqlx::raw_sql("drop schema if exists public cascade; create schema public;").execute(&db.pool).await.expect("reset");
    let mut packages = ferrum_domain::packages().expect("platform packages");
    packages.extend(ferrum_app_erp::packages().expect("ERP's packages"));
    packages.extend(ferrum_app_crm::packages().expect("app packages"));
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
    let k = Kernel::boot(db, catalog, hooks, KernelSettings::default(), None).await.expect("boot");
    ferrum_domain::maintenance(&k).await.expect("maintenance");
    ferrum_app_erp::maintenance(&k).await.expect("maintenance");
    k
}

/// One signed-in user.
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
    async fn cancel(&self, entity: &str, id: Uuid) -> Doc {
        self.k.cancel(&self.ctx, entity, id, None).await.unwrap_or_else(|e| panic!("cancel {entity}: {e:?}"))
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
    async fn report(&self, name: &str, params: &[(&str, String)]) -> Value {
        let p: HashMap<String, String> = params.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
        ferrum_domain::reports::run(&self.k, &self.ctx, name, &p).await.unwrap_or_else(|e| panic!("report {name}: {e:?}"))
    }
}

#[test]
fn crm_end_to_end() {
    // Same runtime shape as the server: unoptimized async hook chains need more than 2 MiB.
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().thread_stack_size(16 << 20).build().unwrap();
    rt.block_on(async { tokio::spawn(scenario()).await.unwrap() });
}

async fn scenario() {
    let k = fresh_kernel().await;
    let tenant = k
        .create_tenant(NewTenant {
            slug: "crm".into(),
            name: "CRM Test Ltd".into(),
            admin_email: "admin@crm.test".into(),
            admin_password: PASSWORD.into(),
            admin_name: None,
            // With ERP: the connector brings taxes and sales orders.
            packages: vec!["crm".into(), "crm_erp".into()],
            settings: Some(json!({"fiscal_year_start_month": 4, "timezone": *ZONE})),
        })
        .await
        .expect("tenant");
    let session = k.login("crm", "admin@crm.test", PASSWORD).await.expect("login");
    let admin = U { k: k.clone(), ctx: k.load_ctx(tenant, session.user, Some(session.token_version), "it".into()).await.expect("ctx") };

    // ───────────── company, tax, items, team ─────────────
    let inr = admin.one("Currency", json!({"code": "INR"})).await;
    let company = admin.insert("Company", json!({"company_name": "CRM Test Ltd", "abbr": "CT", "default_currency": inr["id"]})).await;
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
    let service = uid(&admin
        .insert(
            "Item",
            json!({"item_code": "INSTALL", "item_name": "Installation", "item_group": products["id"], "stock_uom": nos["id"], "is_stock_item": false, "standard_rate": "500"}),
        )
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
    let anna = user("anna@crm.test", "Anna Rep", &["Sales User"]).await;
    let ben = user("ben@crm.test", "Ben Rep", &["Sales User"]).await;
    let maria = user("maria@crm.test", "Maria Manager", &["Sales Manager"]).await;
    assert_eq!(anna.ctx.default_company.map(|c| c.to_string()), Some(cid.clone()), "a single company is everyone's default");

    // Seeded masters.
    let stages = admin.list("SalesStage", json!({})).await;
    assert_eq!(stages.len(), 7);
    let stage = |name: &str| stages.iter().find(|s| s["stage_name"] == json!(name)).map(uid).unwrap().to_string();
    let admin_ref = &admin;
    let reason = |name: &'static str| async move { admin_ref.one("LostReason", json!({"reason": name})).await["id"].clone() };
    let price_reason = reason("Price").await;

    let expo = maria
        .insert("Campaign", json!({"campaign_name": "Expo 2026", "campaign_type": "Trade Show", "status": "Active", "start_date": today().to_string(), "budget": "1000", "actual_cost": "800"}))
        .await;
    refused(anna.k.delete(&anna.ctx, "Campaign", uid(&expo), None).await, "not permitted");
    refused(anna.k.insert(&anna.ctx, "SalesStage", obj(json!({"stage_name": "Demo", "probability": "30"}))).await, "not permitted");

    // ───────────── leads ─────────────
    let l1 = anna
        .insert(
            "Lead",
            json!({"first_name": "Jane", "last_name": "Doe", "organization_name": "Acme Corp", "job_title": "Purchasing Head", "email": "Jane@Acme.test",
                   "phone": "+91 80 1234 5678", "mobile": "+91 98450 00000", "source": "Trade Show", "campaign": expo["id"],
                   "address_line1": "12 MG Road", "city": "Bengaluru", "state": "Karnataka", "postal_code": "560001", "country": "India"}),
        )
        .await;
    let l1_id = uid(&l1);
    assert_eq!(l1["lead_name"], json!("Jane Doe"));
    assert_eq!(l1["email"], json!("jane@acme.test"));
    assert_eq!(l1["status"], json!("New"));
    assert_eq!(l1["assigned_to"], json!("anna@crm.test"), "the creator owns the lead");
    assert!(l1["number"].as_str().unwrap().starts_with("LEAD-"));
    let dup = ben.k.insert(&ben.ctx, "Lead", obj(json!({"first_name": "J", "email": "jane@acme.test"}))).await;
    refused(dup, "already exists");
    refused(anna.k.insert(&anna.ctx, "Lead", obj(json!({"email": "nobody@x.test"}))).await, "first_name");
    let l2 = anna
        .insert(
            "Lead",
            json!({"first_name": "Bob", "last_name": "Stone", "organization_name": "Stone Builders", "email": "bob@stone.test", "source": "Website"}),
        )
        .await;
    let l2_id = uid(&l2);
    let l3 = ben.insert("Lead", json!({"first_name": "Carla", "last_name": "Lee", "email": "carla@lee.test", "source": "Referral"})).await;
    assert_eq!(l3["lead_name"], json!("Carla Lee"));

    // Sales users work their own leads; the manager sees the team's.
    refused(ben.k.get(&ben.ctx, "Lead", l1_id, false).await, "not found");
    refused(ben.k.update(&ben.ctx, "Lead", l1_id, obj(json!({"next_follow_up": today().to_string()})), None).await, "not found");
    assert_eq!(ben.list("Lead", json!({})).await.len(), 1);
    assert_eq!(anna.list("Lead", json!({})).await.len(), 2);
    assert_eq!(maria.list("Lead", json!({})).await.len(), 3);
    refused(maria.k.update(&maria.ctx, "Lead", uid(&l3), obj(json!({"assigned_to": "stranger@x.test"})), None).await, "assigned_to");

    // ───────────── activities ─────────────
    let call = anna.insert("Activity", json!({"activity_type": "Call", "subject": "Intro call", "lead": l1_id.to_string()})).await;
    assert_eq!(call["status"], json!("Open"));
    assert_eq!(anna.get("Lead", l1_id).await["status"], json!("New"), "an open activity is not contact yet");
    let done = anna.action("Activity", uid(&call), "complete", json!({})).await;
    assert_eq!(done["status"], json!("Done"));
    assert!(done["completed_at"].is_string());
    let l1_doc = anna.get("Lead", l1_id).await;
    assert_eq!(l1_doc["status"], json!("Contacted"), "completing an activity on a new lead");
    assert_eq!(l1_doc["last_contacted_on"], json!(today().to_string()));
    let note = anna.insert("Activity", json!({"activity_type": "Note", "subject": "Met at the booth", "lead": l2_id.to_string()})).await;
    assert_eq!(note["status"], json!("Done"), "notes are done when written");
    assert_eq!(anna.get("Lead", l2_id).await["status"], json!("New"), "a note is not an interaction");

    // ───────────── opportunity from a lead ─────────────
    let made = anna.action("Lead", l1_id, "make_opportunity", json!({})).await;
    assert_eq!(made["entity"], json!("Opportunity"));
    let o1 = vid(&made);
    let opp = anna.get("Opportunity", o1).await;
    assert_eq!(opp["party_name"], json!("Acme Corp"));
    assert_eq!(opp["title"], json!("Acme Corp"));
    assert_eq!(opp["sales_stage"], json!(stage("Prospecting")), "first open stage");
    assert_eq!(dec(&opp["probability"]), n("10"), "the stage's probability");
    assert_eq!(opp["campaign"], expo["id"]);
    assert_eq!(opp["source"], json!("Trade Show"));
    assert_eq!(opp["currency"], inr["id"]);
    let l1_doc = anna.get("Lead", l1_id).await;
    assert_eq!(l1_doc["status"], json!("Qualified"), "an opportunity qualifies the lead");
    assert_eq!(l1_doc["qualified_by"], json!("anna@crm.test"));

    let opp = anna
        .update(
            "Opportunity",
            o1,
            json!({"sales_stage": stage("Proposal"), "expected_close_date": today().to_string(),
                   "items": [{"item": widget, "qty": "10", "rate": "150"}, {"item": service, "qty": "1", "rate": "500"}],
                   "competitors": [{"competitor": "Globex", "their_price": "1900"}]}),
        )
        .await;
    assert_eq!(dec(&opp["amount"]), n("2000"), "amount from the items");
    assert_eq!(dec(&opp["probability"]), n("60"));
    assert_eq!(dec(&opp["weighted_amount"]), n("1200"));
    assert_eq!(dec(&opp["base_weighted_amount"]), n("1200"));
    refused(anna.k.update(&anna.ctx, "Opportunity", o1, obj(json!({"probability": "120"})), None).await, "probability");
    let opp = anna.update("Opportunity", o1, json!({"sales_stage": stage("Negotiation"), "probability": "75"})).await;
    assert_eq!(dec(&opp["probability"]), n("75"), "a probability set with the stage change stands");
    assert_eq!(dec(&opp["weighted_amount"]), n("1500"));
    // Choosing the Lost stage closes the opportunity as lost, which needs a reason.
    refused(anna.k.update(&anna.ctx, "Opportunity", o1, obj(json!({"sales_stage": stage("Lost")})), None).await, "lost_reason");
    refused(ben.k.get(&ben.ctx, "Opportunity", o1, false).await, "not found");

    // ───────────── quotation for the lead ─────────────
    let q1 = vid(&anna.action("Opportunity", o1, "make_quotation", json!({})).await);
    let q = anna.get("Quotation", q1).await;
    assert_eq!(q["party_type"], json!("Lead"));
    assert_eq!(q["lead"], json!(l1_id.to_string()));
    assert_eq!(q["opportunity"], json!(o1.to_string()));
    assert_eq!(q["party_name"], json!("Acme Corp"));
    assert_eq!(q["valid_till"], json!((today() + chrono::Duration::days(30)).to_string()), "30 days validity by default");
    assert_eq!(dec(&q["net_total"]), n("2000"));
    assert_eq!(dec(&q["total_taxes"]), n("200"), "the default sales tax template applies");
    assert_eq!(dec(&q["grand_total"]), n("2200"));
    assert_eq!(q["status"], json!("Draft"));
    assert_eq!(q["assigned_to"], json!("anna@crm.test"), "the opportunity's owner owns its quotation");
    let q = anna.submit("Quotation", q1).await;
    assert_eq!(q["status"], json!("Open"));
    assert!(q["number"].as_str().unwrap().starts_with("QTN-"));
    refused(anna.k.run_action(&anna.ctx, "Quotation", q1, "make_sales_order", json!({})).await, "convert the lead");
    refused(ben.k.get(&ben.ctx, "Quotation", q1, false).await, "not found");
    let printed = k.render_print(&anna.ctx, "Quotation", q1).await.expect("print");
    for text in ["CRM Test Ltd", "Acme Corp", q["number"].as_str().unwrap(), "Installation", "VAT 10%", "2,200.00", "INR"] {
        assert!(printed.contains(text), "the printed quotation shows {text}");
    }

    // ───────────── lead conversion ─────────────
    let converted = anna.action("Lead", l1_id, "convert_to_customer", json!({})).await;
    assert_eq!(converted["entity"], json!("Customer"));
    let acme = vid(&converted).to_string();
    let customer = anna.get("Customer", vid(&converted)).await;
    assert_eq!(customer["customer_name"], json!("Acme Corp"));
    assert_eq!(customer["customer_type"], json!("Company"));
    assert_eq!(customer["email"], json!("jane@acme.test"));
    assert_eq!(customer["billing_address"], json!("12 MG Road\nBengaluru, Karnataka 560001\nIndia"), "the primary address lands on the customer");
    let contact = anna.get("Contact", vid(&json!({"id": converted["contact"]}))).await;
    assert_eq!(contact["full_name"], json!("Jane Doe"));
    assert_eq!(contact["email"], json!("jane@acme.test"));
    assert_eq!(contact["designation"], json!("Purchasing Head"));
    assert_eq!(contact["customer"], json!(acme));
    assert_eq!(contact["phones"].as_array().unwrap().len(), 2);
    let address = anna.get("Address", vid(&json!({"id": converted["address"]}))).await;
    assert_eq!(address["is_primary"], json!(true));
    assert_eq!(address["address_title"], json!("Acme Corp"));
    let l1_doc = anna.get("Lead", l1_id).await;
    assert_eq!(l1_doc["status"], json!("Converted"));
    assert_eq!(l1_doc["customer"], json!(acme));
    assert_eq!(l1_doc["contact"], converted["contact"]);
    assert_eq!(l1_doc["converted_on"], json!(today().to_string()));
    assert_eq!(anna.get("Opportunity", o1).await["customer"], json!(acme), "the lead's opportunity follows it");
    assert_eq!(anna.get("Quotation", q1).await["customer"], json!(acme), "so does its quotation");
    refused(anna.k.run_action(&anna.ctx, "Lead", l1_id, "convert_to_customer", json!({})).await, "not available");
    refused(anna.k.update(&anna.ctx, "Lead", l1_id, obj(json!({"status": "Qualified"})), None).await, "converted_via_action");

    // ───────────── ordering the quotation ─────────────
    let so1 = vid(&anna.action("Quotation", q1, "make_sales_order", json!({})).await);
    let so = anna.get("SalesOrder", so1).await;
    assert_eq!(so["customer"], json!(acme));
    assert_eq!(so["crm_quotation"], json!(q1.to_string()));
    assert_eq!(dec(&so["grand_total"]), n("2200"), "the order prices exactly like the quotation");
    let lines = so["items"].as_array().unwrap().clone();
    assert!(lines.iter().all(|l| l["crm_quotation"] == json!(q1.to_string()) && l["crm_quotation_line"].is_string()));
    // The customer takes 6 widgets for now.
    let so = anna.update("SalesOrder", so1, json!({"items": [{"id": lines[0]["id"], "qty": "6"}, {"id": lines[1]["id"]}]})).await;
    assert_eq!(dec(&so["grand_total"]), n("1540"));
    anna.submit("SalesOrder", so1).await;
    let q = anna.get("Quotation", q1).await;
    assert_eq!(q["status"], json!("Partially Ordered"));
    assert_eq!(dec(&q["per_ordered"]), n("63.64"), "7 of 11 units");
    assert_eq!(dec(&q["items"][0]["ordered_qty"]), n("6"));
    let opp = anna.get("Opportunity", o1).await;
    assert_eq!(opp["status"], json!("Won"), "an ordered quotation wins its opportunity");
    assert_eq!(opp["sales_stage"], json!(stage("Won")));
    assert_eq!(dec(&opp["weighted_amount"]), n("2000"));
    assert_eq!(opp["closed_on"], json!(today().to_string()));
    refused(anna.k.cancel(&anna.ctx, "Quotation", q1, None).await, "cancel the sales orders");

    // Cancelling the order gives the quantities back and reopens the deal.
    anna.cancel("SalesOrder", so1).await;
    let q = anna.get("Quotation", q1).await;
    assert_eq!(q["status"], json!("Open"));
    assert_eq!(dec(&q["per_ordered"]), Decimal::ZERO);
    assert_eq!(dec(&q["items"][0]["ordered_qty"]), Decimal::ZERO);
    let opp = anna.get("Opportunity", o1).await;
    assert_eq!(opp["status"], json!("Open"));
    assert_eq!(opp["sales_stage"], json!(stage("Negotiation")), "back in the last open stage");
    assert_eq!(dec(&opp["probability"]), n("80"));
    assert!(opp["closed_on"].is_null());

    // The amendment still counts against the quotation.
    let amended = k.amend(&anna.ctx, "SalesOrder", so1).await.expect("amend");
    assert_eq!(amended["crm_quotation"], json!(q1.to_string()));
    assert!(amended["items"].as_array().unwrap().iter().all(|l| l["crm_quotation_line"].is_string()));
    anna.submit("SalesOrder", uid(&amended)).await;
    assert_eq!(anna.get("Quotation", q1).await["status"], json!("Partially Ordered"));
    assert_eq!(anna.get("Opportunity", o1).await["status"], json!("Won"));

    // The rest of the quotation: only the 4 open widgets.
    let so2 = vid(&anna.action("Quotation", q1, "make_sales_order", json!({})).await);
    let so = anna.get("SalesOrder", so2).await;
    let lines = so["items"].as_array().unwrap();
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0]["item"], json!(widget));
    assert_eq!(dec(&lines[0]["qty"]), n("4"));
    assert_eq!(dec(&so["grand_total"]), n("660"));
    anna.submit("SalesOrder", so2).await;
    let q = anna.get("Quotation", q1).await;
    assert_eq!(q["status"], json!("Ordered"));
    assert_eq!(dec(&q["per_ordered"]), n("100"));
    refused(anna.k.run_action(&anna.ctx, "Quotation", q1, "make_sales_order", json!({})).await, "not available");

    // Two drafts for the same quotation cannot both be ordered.
    let q3 = anna.insert("Quotation", json!({"party_type": "Customer", "customer": acme, "items": [{"item": widget, "qty": "2"}]})).await;
    assert_eq!(dec(&q3["total_taxes"]), n("30"), "the default tax template applies on the first save (no india package)");
    assert_eq!(dec(&q3["grand_total"]), n("330"), "standard rate 150 × 2 + 10% VAT");
    let q3 = uid(&q3);
    anna.submit("Quotation", q3).await;
    let so_a = vid(&anna.action("Quotation", q3, "make_sales_order", json!({})).await);
    let so_b = vid(&anna.action("Quotation", q3, "make_sales_order", json!({})).await);
    anna.submit("SalesOrder", so_a).await;
    refused(anna.k.submit(&anna.ctx, "SalesOrder", so_b, None).await, "exceed");
    assert_eq!(anna.get("Quotation", q3).await["status"], json!("Ordered"));

    // ───────────── losing ─────────────
    // What the actions read, their pages ask for: a quotation cannot be lost without a reason, and
    // a lead can be converted into a customer who exists.
    let asks = |entity: &str, action: &str| -> Vec<String> {
        let ent = anna.ctx.schema.entity(entity).unwrap();
        ent.def.actions.iter().find(|a| a.name == action).unwrap().params.iter().map(|p| p.name.clone()).collect()
    };
    for (entity, action, names) in [
        ("Quotation", "mark_lost", &["lost_reason", "lost_detail"][..]),
        ("Opportunity", "mark_lost", &["lost_reason", "lost_detail"]),
        ("Lead", "convert_to_customer", &["customer", "contact", "customer_group", "create_new"]),
    ] {
        assert_eq!(asks(entity, action), names, "{entity}.{action}");
    }
    let o2 = vid(&anna.action("Lead", l2_id, "make_opportunity", json!({})).await);
    refused(anna.k.run_action(&anna.ctx, "Opportunity", o2, "mark_lost", json!({})).await, "lost_reason");
    refused(anna.k.run_action(&anna.ctx, "Opportunity", o2, "mark_lost", json!({"lost_reason": "Too Expensive"})).await, "lost_reason");
    let lost = anna.action("Opportunity", o2, "mark_lost", json!({"lost_reason": "Price", "lost_detail": "Went with a cheaper local firm"})).await;
    assert_eq!(lost["status"], json!("Lost"));
    let opp = anna.get("Opportunity", o2).await;
    assert_eq!(opp["sales_stage"], json!(stage("Lost")));
    assert_eq!(dec(&opp["probability"]), Decimal::ZERO);
    assert_eq!(opp["lost_reason"], price_reason);
    refused(anna.k.update(&anna.ctx, "Opportunity", o2, obj(json!({"status": "Open"})), None).await, "open stage");
    refused(anna.k.update(&anna.ctx, "Lead", l2_id, obj(json!({"status": "Lost"})), None).await, "lost_reason");
    anna.update("Lead", l2_id, json!({"status": "Lost", "lost_reason": reason("No Budget").await})).await;
    refused(anna.k.run_action(&anna.ctx, "Lead", l2_id, "make_opportunity", json!({})).await, "not available");
    refused(anna.k.insert(&anna.ctx, "Opportunity", obj(json!({"lead": l2_id.to_string()}))).await, "reopen it first");

    // A deal lost with its only quotation.
    let o3 = anna.insert("Opportunity", json!({"opportunity_from": "Customer", "customer": acme, "title": "Acme spares", "amount": "500"})).await;
    let o3 = uid(&o3);
    let q6 = vid(&anna.action("Opportunity", o3, "make_quotation", json!({})).await);
    refused(anna.k.submit(&anna.ctx, "Quotation", q6, None).await, "has_items");
    anna.update("Quotation", q6, json!({"items": [{"item": service, "qty": "1"}]})).await;
    anna.submit("Quotation", q6).await;
    refused(anna.k.run_action(&anna.ctx, "Quotation", q6, "mark_lost", json!({})).await, "lost_reason");
    anna.action("Quotation", q6, "mark_lost", json!({"lost_reason": price_reason})).await;
    assert_eq!(anna.get("Quotation", q6).await["status"], json!("Lost"));
    let opp = anna.get("Opportunity", o3).await;
    assert_eq!(opp["status"], json!("Lost"), "the opportunity is lost with its last quotation");
    assert_eq!(opp["lost_reason"], price_reason);

    // ───────────── validity and expiry ─────────────
    let line = json!([{"item": widget, "qty": "1"}]);
    refused(
        anna.k
            .insert(&anna.ctx, "Quotation", obj(json!({"customer": acme, "items": line, "valid_till": (today() - chrono::Duration::days(1)).to_string()})))
            .await,
        "validity_after_date",
    );
    let q4 = uid(&anna.insert("Quotation", json!({"customer": acme, "items": line, "valid_till": today().to_string()})).await);
    anna.submit("Quotation", q4).await;
    ferrum_app_crm::maintenance(&k).await.expect("maintenance");
    assert_eq!(anna.get("Quotation", q4).await["status"], json!("Open"), "valid through its last day");
    // A day passes.
    let mut tx = k.db.tenant_tx(tenant).await.unwrap();
    sqlx::query("update e_quotation set valid_till = $3 where tenant_id = $1 and id = $2")
        .bind(tenant)
        .bind(q4)
        .bind(today() - chrono::Duration::days(1))
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    refused(anna.k.run_action(&anna.ctx, "Quotation", q4, "make_sales_order", json!({})).await, "expired");
    ferrum_app_crm::maintenance(&k).await.expect("maintenance");
    ferrum_app_crm::maintenance(&k).await.expect("maintenance runs again");
    assert_eq!(anna.get("Quotation", q4).await["status"], json!("Expired"));
    assert_eq!(anna.get("Quotation", q1).await["status"], json!("Ordered"), "ordered quotations never expire");
    let mut tx = k.db.tenant_tx(tenant).await.unwrap();
    let expiries: i64 = sqlx::query_scalar("select count(*) from audit_event where tenant_id = $1 and record_id = $2 and action = 'expire'")
        .bind(tenant)
        .bind(q4)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    drop(tx);
    assert_eq!(expiries, 1, "expiry is idempotent");
    refused(anna.k.run_action(&anna.ctx, "Quotation", q4, "make_sales_order", json!({})).await, "expired");
    let backdated = anna
        .insert(
            "Quotation",
            json!({"customer": acme, "items": line, "posting_date": (today() - chrono::Duration::days(40)).to_string(),
                   "valid_till": (today() - chrono::Duration::days(10)).to_string()}),
        )
        .await;
    assert_eq!(anna.submit("Quotation", uid(&backdated)).await["status"], json!("Expired"), "entered after its validity");

    // ───────────── reassignment ─────────────
    maria.update("Lead", l2_id, json!({"assigned_to": "ben@crm.test"})).await;
    assert_eq!(ben.get("Lead", l2_id).await["assigned_to"], json!("ben@crm.test"), "the new assignee sees it");
    assert_eq!(anna.get("Lead", l2_id).await["created_by"], json!(anna.ctx.user.to_string()), "its creator still does, and stays its creator");

    // ───────────── contacts and addresses ─────────────
    let sam = anna.insert("Contact", json!({"first_name": "Sam", "customer": acme, "emails": [{"email": "sam@acme.test"}, {"email": "s.k@acme.test"}]})).await;
    assert_eq!(sam["email"], json!("sam@acme.test"), "the first email is primary");
    assert_eq!(sam["emails"][0]["is_primary"], json!(true));
    let two = anna
        .k
        .insert(
            &anna.ctx,
            "Contact",
            obj(json!({"first_name": "X", "emails": [{"email": "a@x.test", "is_primary": true}, {"email": "b@x.test", "is_primary": true}]})),
        )
        .await;
    assert!(matches!(&two, Err(Error::Validation(e)) if e.iter().any(|f| f.row == Some(("emails".into(), 1)))), "{two:?}");
    refused(
        anna.k.insert(&anna.ctx, "Contact", obj(json!({"first_name": "Y", "emails": [{"email": "a@x.test"}, {"email": "A@x.test"}]}))).await,
        "listed twice",
    );
    refused(anna.k.insert(&anna.ctx, "Address", obj(json!({"address_line1": "1 Nowhere", "city": "Pune"}))).await, "linked");
    let hq = anna.insert("Address", json!({"address_line1": "99 New Street", "city": "Pune", "is_primary": true, "customer": acme})).await;
    assert_eq!(hq["address_title"], json!("Acme Corp"));
    assert_eq!(anna.get("Address", vid(&json!({"id": converted["address"]}))).await["is_primary"], json!(false), "one primary billing address");
    assert_eq!(anna.get("Customer", vid(&json!({"id": acme}))).await["billing_address"], json!("99 New Street\nPune"));

    // ───────────── pipeline and reports ─────────────
    ben.insert(
        "Opportunity",
        json!({"opportunity_from": "Customer", "customer": acme, "title": "Acme service plan", "amount": "1000", "sales_stage": stage("Qualification")}),
    )
    .await;
    let o5 = anna.insert("Opportunity", json!({"opportunity_from": "Customer", "customer": acme, "amount": "3000", "sales_stage": stage("Proposal")})).await;
    assert_eq!(o5["title"], json!("Acme Corp"));
    assert_eq!(dec(&o5["weighted_amount"]), n("1800"));

    let company_param = ("company", cid.clone());
    let pipe = maria.report("sales_pipeline", std::slice::from_ref(&company_param)).await;
    assert_eq!(pipe["totals"]["count"], json!(2));
    assert_eq!(dec(&pipe["totals"]["amount"]), n("4000"));
    assert_eq!(dec(&pipe["totals"]["weighted_amount"]), n("2000"));
    let rows = pipe["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 5, "every open stage");
    let proposal = rows.iter().find(|r| r["sales_stage"] == json!("Proposal")).unwrap();
    assert_eq!((proposal["count"].clone(), dec(&proposal["weighted_amount"])), (json!(1), n("1800")));
    let mine = anna.report("sales_pipeline", std::slice::from_ref(&company_param)).await;
    assert_eq!((mine["totals"]["count"].clone(), dec(&mine["totals"]["amount"])), (json!(1), n("3000")), "a rep's pipeline is their own");
    let bens = maria.report("sales_pipeline", &[company_param.clone(), ("assigned_to", "Ben@crm.test".into())]).await;
    assert_eq!(dec(&bens["totals"]["amount"]), n("1000"));

    let funnel = maria.report("lead_funnel", std::slice::from_ref(&company_param)).await;
    let t = &funnel["totals"];
    assert_eq!(
        (t["leads"].clone(), t["contacted"].clone(), t["qualified"].clone(), t["converted"].clone(), t["lost"].clone()),
        (json!(3), json!(2), json!(2), json!(1), json!(1))
    );
    assert_eq!(dec(&t["conversion_pct"]), n("33.33"));
    let groups: Vec<&str> = funnel["rows"].as_array().unwrap().iter().map(|r| r["group"].as_str().unwrap()).collect();
    assert_eq!(groups, ["Referral", "Trade Show", "Website"]);
    let by_campaign = maria.report("lead_funnel", &[company_param.clone(), ("group_by", "Campaign".into())]).await;
    let expo_row = by_campaign["rows"].as_array().unwrap().iter().find(|r| r["group"] == json!("Expo 2026")).cloned().unwrap();
    assert_eq!((expo_row["leads"].clone(), dec(&expo_row["conversion_pct"])), (json!(1), n("100")));

    let camp = maria.report("campaign_performance", std::slice::from_ref(&company_param)).await;
    let row = &camp["rows"][0];
    assert_eq!(row["campaign"], json!("Expo 2026"));
    assert_eq!(
        (row["leads"].clone(), row["converted_leads"].clone(), row["opportunities"].clone(), row["won"].clone()),
        (json!(1), json!(1), json!(1), json!(1))
    );
    assert_eq!(dec(&row["won_amount"]), n("2000"));
    assert_eq!(dec(&row["cost_per_lead"]), n("800"));
    assert_eq!(dec(&row["roi_pct"]), n("150"), "(2000 won - 800 spent) / 800");

    // Attribution: Jane's deal (her lead, its opportunity and its quotation all Expo's) is Expo's
    // under every model. A webinar's lead whose deal Expo then took on is shared by the model.
    let webinar =
        maria.insert("Campaign", json!({"campaign_name": "Webinar", "campaign_type": "Webinar", "status": "Active", "start_date": today().to_string()})).await;
    let l4 = anna
        .insert("Lead", json!({"first_name": "Dev", "last_name": "Rao", "organization_name": "Rao Works", "email": "dev@rao.test", "campaign": webinar["id"]}))
        .await;
    let o4 = vid(&anna.action("Lead", uid(&l4), "make_opportunity", json!({})).await);
    anna.update("Opportunity", o4, json!({"campaign": expo["id"], "amount": "300"})).await;
    let won = anna.update("Opportunity", o4, json!({"sales_stage": stage("Won")})).await;
    assert_eq!((won["status"].clone(), dec(&won["base_amount"])), (json!("Won"), n("300")));
    let attributed = |r: &Value, name: &str| {
        r["rows"].as_array().unwrap().iter().find(|x| x["campaign"] == json!(name)).map(|x| (x["deals_touched"].as_i64().unwrap(), dec(&x["revenue"])))
    };
    for (model, expo_gets, webinar_gets) in [("Linear", "2150", Some("150")), ("First Touch", "2000", Some("300")), ("Last Touch", "2300", None)] {
        let r = maria.report("campaign_attribution", &[company_param.clone(), ("model", model.into())]).await;
        assert_eq!(attributed(&r, "Expo 2026").map(|a| a.1), Some(n(expo_gets)), "{model}: {r}");
        assert_eq!(attributed(&r, "Webinar").map(|a| a.1), webinar_gets.map(n), "{model}: {r}");
        assert_eq!(r["meta"]["model"], json!(model));
    }
    let linear = maria.report("campaign_attribution", std::slice::from_ref(&company_param)).await;
    assert_eq!(attributed(&linear, "Expo 2026").map(|a| a.0), Some(2), "both deals touched by Expo");
    let total: rust_decimal::Decimal = linear["rows"].as_array().unwrap().iter().map(|x| dec(&x["revenue"])).sum();
    assert_eq!(total, dec(&linear["totals"]["revenue"]), "the shares add up to what was won");
    assert!(ben.k.get(&ben.ctx, "Opportunity", o4, false).await.is_err(), "Ben reads none of these deals");
    let bens = ben.report("campaign_attribution", std::slice::from_ref(&company_param)).await;
    assert_eq!(bens["totals"]["won_deals"], json!(0), "and his report counts none of them");

    let reg = maria.report("quotation_register", std::slice::from_ref(&company_param)).await;
    let t = &reg["totals"];
    assert_eq!(t["count"], json!(5), "Q1, Q3, Q6, the expired and the backdated one");
    assert_eq!(dec(&t["quoted"]), n("3410"));
    assert_eq!(dec(&t["ordered"]), n("2530"), "1540 + 660 from Q1, 330 from Q3; the cancelled order does not count");
    assert_eq!(dec(&t["lost"]), n("550"));
    assert_eq!(dec(&t["expired"]), n("330"));
    assert_eq!(dec(&t["conversion_pct"]), n("74.19"));
    assert_eq!(dec(&t["win_rate_pct"]), n("40"));
    let q1_row = reg["rows"].as_array().unwrap().iter().find(|r| r["quotation_id"] == json!(q1.to_string())).cloned().unwrap();
    assert_eq!((q1_row["status"].clone(), dec(&q1_row["ordered_amount"])), (json!("Ordered"), n("2200")));
    let lost_only = maria.report("quotation_register", &[company_param.clone(), ("status", "Lost".into())]).await;
    assert_eq!(lost_only["totals"]["count"], json!(1));

    // ───────────── own records: created or assigned ─────────────
    let dana = maria.insert("Lead", json!({"first_name": "Dana", "organization_name": "Initech", "assigned_to": "anna@crm.test"})).await;
    let dana_id = uid(&dana);
    assert_eq!(anna.get("Lead", dana_id).await["created_by"], json!(maria.ctx.user.to_string()), "the rep sees it; the manager stays its creator");
    let edited = anna.update("Lead", dana_id, json!({"next_follow_up": today().to_string(), "status": "Contacted"})).await;
    assert_eq!(edited["status"], json!("Contacted"), "and the rep can work it");
    assert!(anna.list("Lead", json!({})).await.iter().any(|l| l["id"] == dana["id"]), "it is on the rep's list");
    refused(ben.k.get(&ben.ctx, "Lead", dana_id, false).await, "not found");
    assert!(!ben.list("Lead", json!({})).await.iter().any(|l| l["id"] == dana["id"]), "other reps still cannot see it");

    // Reports need read access to their entity.
    let clerk = user("clerk@crm.test", "Accounts Clerk", &["Accounts User"]).await;
    let denied = ferrum_domain::reports::run(&clerk.k, &clerk.ctx, "sales_pipeline", &HashMap::new()).await;
    assert!(matches!(denied, Err(Error::Forbidden(_))), "{denied:?}");

    // ───────────── several leads of one prospect become one customer ─────────────
    let globex = maria.insert("Prospect", json!({"prospect_name": "Globex Inc", "industry": "Manufacturing"})).await;
    let pat = maria.insert("Lead", json!({"first_name": "Pat", "prospect": globex["id"], "email": "pat@globex.test"})).await;
    assert_eq!(pat["organization_name"], json!("Globex Inc"), "the organization comes from the prospect");
    let lou = maria.insert("Lead", json!({"first_name": "Lou", "last_name": "Grant", "prospect": globex["id"]})).await;
    let pats_call = maria.insert("Activity", json!({"activity_type": "Call", "subject": "Discovery call", "status": "Done", "lead": pat["id"]})).await;
    let lous_card =
        maria.insert("Contact", json!({"first_name": "Lou", "last_name": "Grant", "lead": lou["id"], "emails": [{"email": "lou@globex.test"}]})).await;
    let first = maria.action("Lead", uid(&pat), "convert_to_customer", json!({})).await;
    assert_eq!(maria.get("Activity", uid(&pats_call)).await["customer"], first["id"], "the lead's history moves to the customer");
    assert_eq!(maria.get("Prospect", uid(&globex)).await["customer"], first["id"], "the prospect became that customer");
    let second = maria.action("Lead", uid(&lou), "convert_to_customer", json!({})).await;
    assert_eq!(second["id"], first["id"], "the second lead joins the same customer");
    assert!(second["address"].is_null(), "no address was captured");
    assert_eq!(second["contact"], lous_card["id"], "the contact kept for the lead is reused, not duplicated");
    assert_eq!(maria.get("Contact", uid(&lous_card)).await["customer"], first["id"]);
    assert_eq!(maria.list("Customer", json!({"customer_name": "Globex Inc"})).await.len(), 1);
    assert_eq!(maria.list("Contact", json!({"customer": first["id"]})).await.len(), 2);

    // ───────────── foreign-currency opportunities count in company currency ─────────────
    let usd = admin.one("Currency", json!({"code": "USD"})).await;
    let yesterday = (today() - chrono::Duration::days(1)).to_string();
    admin.insert("ExchangeRate", json!({"from_currency": usd["id"], "to_currency": inr["id"], "rate": "83", "valid_from": yesterday})).await;
    let export = maria
        .insert(
            "Opportunity",
            json!({"opportunity_from": "Customer", "customer": first["id"], "currency": usd["id"], "amount": "100", "sales_stage": stage("Proposal")}),
        )
        .await;
    assert_eq!(dec(&export["conversion_rate"]), n("83"));
    assert_eq!((dec(&export["weighted_amount"]), dec(&export["base_amount"]), dec(&export["base_weighted_amount"])), (n("60"), n("8300"), n("4980")));
    let export = maria.update("Opportunity", uid(&export), json!({"conversion_rate": "84"})).await;
    assert_eq!(dec(&export["base_amount"]), n("8400"), "a rate the user gives stands");
    let export = maria.update("Opportunity", uid(&export), json!({"currency": inr["id"]})).await;
    assert_eq!((dec(&export["conversion_rate"]), dec(&export["base_amount"])), (n("1"), n("100")));

    // ── a personal-data erasure takes a contact's names and every email and phone ──
    let kim = admin
        .insert(
            "Contact",
            json!({"first_name": "Kim", "last_name": "Private", "emails": [{"email": "kim@private.test"}], "phones": [{"phone": "+1 555 0199"}]}),
        )
        .await;
    let found = k.personal_data(&admin.ctx, "kim@private.test").await.unwrap();
    assert!(found["records"].as_array().unwrap().iter().any(|r| r["entity"] == json!("Contact")), "{found}");
    k.erase_personal_data(&admin.ctx, "kim@private.test", "Request 7").await.unwrap();
    let kim = admin.get("Contact", uid(&kim)).await;
    assert!(kim["first_name"].as_str().unwrap().starts_with("Erased ") && kim["last_name"].is_null() && kim["email"].is_null(), "{kim:?}");
    assert!(kim["emails"].as_array().unwrap().is_empty() && kim["phones"].as_array().unwrap().is_empty());

    // ── duplicate leads become one, and what hung off them stays ──
    let first =
        anna.insert("Lead", json!({"first_name": "Dana", "last_name": "Twice", "email": "dana@twice.test", "mobile": "+91 98765 00001", "company": cid})).await;
    // The same mobile is the same person: refused, unless kept on purpose, and then linked to merge.
    let twice = json!({"first_name": "D.", "last_name": "Twice", "email": "dana.work@twice.test", "mobile": "+91 98765 00001", "company": cid});
    refused(anna.k.insert(&anna.ctx, "Lead", obj(twice.clone())).await, "already exists");
    let mut kept = twice.clone();
    kept["allow_duplicate"] = json!(true);
    let again = anna.insert("Lead", kept).await;
    assert_eq!(again["duplicate_of"], first["id"], "linked to the lead it duplicates");
    assert_eq!(again["allow_duplicate"], json!(false), "keeping one is decided save by save");
    let groups = k.duplicates(&maria.ctx, "Lead", 50).await.unwrap();
    assert!(groups.iter().any(|g| g["ids"].as_array().is_some_and(|ids| ids.contains(&again["id"]) && ids.contains(&first["id"]))), "{groups:?}");
    let deal = anna
        .insert(
            "Opportunity",
            json!({"title": "Their deal", "opportunity_from": "Lead", "lead": again["id"], "company": cid, "sales_stage": stage("Proposal")}),
        )
        .await;
    k.merge_records(&maria.ctx, "Lead", uid(&first), uid(&again), "The same person, entered twice").await.expect("merged");
    assert_eq!(anna.get("Opportunity", uid(&deal)).await["lead"], first["id"], "their deal comes along");
    assert_eq!(anna.get("Lead", uid(&again)).await["id"], first["id"], "the old id answers with what it became");

    // ── a forecast by month, weighted, and honest about what has no date ──
    let month = today().format("%Y-%m").to_string();
    let by_month = [company_param.clone(), ("from_date", today().to_string()), ("to_date", today().to_string()), ("group_by", "Month".into())];
    let forecast = maria.report("sales_forecast", &by_month).await;
    let row = forecast["rows"].as_array().unwrap().iter().find(|r| r["group"] == json!(month)).cloned();
    assert!(row.is_some(), "{forecast}");
    assert!(forecast["meta"]["without_a_date"].as_i64().unwrap() >= 1, "their deal has no date: counted apart, {forecast}");

    // ── big discounts and totals need someone else ──
    admin
        .insert("QuotationApprovalRule", json!({"rule_name": "Big ones", "discount_over": "10", "total_over": "100000", "approver_role": "Sales Manager"}))
        .await;
    let quote = anna.insert("Quotation", json!({"company": cid, "party_type": "Customer", "customer": acme, "items": [{"item": widget, "qty": "10", "rate": "100"}], "discount_percent": "5"})).await;
    assert_eq!(quote["needs_approval"], json!(false), "within the limits");
    let q = uid(&quote);
    let quote = anna.update("Quotation", q, json!({"discount_percent": "20"})).await;
    assert_eq!(quote["needs_approval"], json!(true));
    refused(anna.k.submit(&anna.ctx, "Quotation", q, None).await, "approval");
    refused(anna.k.run_action(&anna.ctx, "Quotation", q, "approve_quotation", json!({})).await, "someone else");
    refused(ben.k.run_action(&ben.ctx, "Quotation", q, "approve_quotation", json!({})).await, "not found");
    maria.action("Quotation", q, "approve_quotation", json!({})).await;
    let quote = anna.get("Quotation", q).await;
    assert_eq!(quote["approved_by"], json!("maria@crm.test"));
    // The number approved is the number that goes out.
    let quote = anna.update("Quotation", q, json!({"discount_percent": "30"})).await;
    assert!(quote["approved_by"].is_null(), "another discount is another decision");
    refused(anna.k.submit(&anna.ctx, "Quotation", q, None).await, "approval");
    maria.action("Quotation", q, "approve_quotation", json!({})).await;
    assert_eq!(anna.submit("Quotation", q).await["docstatus"], json!(1), "approved, and unchanged");

    // ── an approval is of the terms, and what is ordered and billed keeps them (FEO-025) ──
    let rush = uid(&anna
        .insert(
            "Quotation",
            json!({"company": cid, "party_type": "Customer", "customer": acme, "items": [{"item": widget, "qty": "10", "rate": "100"}], "discount_percent": "20"}),
        )
        .await);
    maria.action("Quotation", rush, "approve_quotation", json!({})).await;
    assert!(anna.get("Quotation", rush).await["approved_digest"].as_str().is_some_and(|d| d.len() == 64), "approved as its terms");
    // The same total and discount on other terms is another decision.
    let later = (today() + chrono::Duration::days(60)).to_string();
    assert!(anna.update("Quotation", rush, json!({"valid_till": later})).await["approved_by"].is_null(), "another validity");
    maria.action("Quotation", rush, "approve_quotation", json!({})).await;
    assert!(anna.update("Quotation", rush, json!({"terms": "Delivered within 2 days"})).await["approved_by"].is_null(), "other terms");
    maria.action("Quotation", rush, "approve_quotation", json!({})).await;
    anna.submit("Quotation", rush).await;
    // The order keeps the quoted price: another is refused until it is the quoted one again.
    let order = vid(&anna.action("Quotation", rush, "make_sales_order", json!({})).await);
    let line = anna.get("SalesOrder", order).await["items"][0].clone();
    anna.update("SalesOrder", order, json!({"items": [{"id": line["id"], "discount_percent": "50"}]})).await;
    refused(anna.k.submit(&anna.ctx, "SalesOrder", order, None).await, "not what quotation");
    anna.update("SalesOrder", order, json!({"items": [{"id": line["id"], "discount_percent": line["discount_percent"]}], "discount_percent": "25"})).await;
    refused(anna.k.submit(&anna.ctx, "SalesOrder", order, None).await, "additional discount");
    anna.update("SalesOrder", order, json!({"discount_percent": "20"})).await;
    anna.submit("SalesOrder", order).await;
    assert_eq!(anna.get("Quotation", rush).await["status"], json!("Ordered"));
    // So does its invoice; a partial credit note gives back what was billed, at what was billed.
    let accountant = user("accountant@crm.test", "Ada Accountant", &["Accounts Manager"]).await;
    let invoice = vid(&accountant.action("SalesOrder", order, "make_sales_invoice", json!({})).await);
    let line = accountant.get("SalesInvoice", invoice).await["items"][0].clone();
    accountant.update("SalesInvoice", invoice, json!({"items": [{"id": line["id"], "discount_percent": "50"}]})).await;
    refused(accountant.k.submit(&accountant.ctx, "SalesInvoice", invoice, None).await, "not what quotation");
    accountant.update("SalesInvoice", invoice, json!({"items": [{"id": line["id"], "discount_percent": line["discount_percent"]}]})).await;
    let billed = dec(&accountant.submit("SalesInvoice", invoice).await["grand_total"]);
    let quoted = dec(&anna.get("Quotation", rush).await["grand_total"]);
    assert_eq!(billed, quoted, "billed as quoted: 10 at the item's rate, less 20%, with 10% VAT");
    let credit = vid(&accountant.action("SalesInvoice", invoice, "make_credit_note", json!({})).await);
    let line = accountant.get("SalesInvoice", credit).await["items"][0].clone();
    accountant.update("SalesInvoice", credit, json!({"items": [{"id": line["id"], "qty": "-3"}]})).await;
    let returned = dec(&accountant.submit("SalesInvoice", credit).await["grand_total"]);
    assert_eq!(billed + returned, quoted * n("0.7"), "7 kept, at the quoted price, discount and taxes");
    assert_eq!(anna.get("Quotation", rush).await["status"], json!("Ordered"), "a return is not an order taken back");

    // ───────────── product rules ─────────────
    // What may be quoted together: an item that needs another, two that exclude each other, so many
    // of one per unit of another. A draft says what it breaks, and is not submitted until nothing.
    let item = |code: &str, name: &str, rate: &str| {
        let (admin, group, uom) = (&admin, products["id"].clone(), nos["id"].clone());
        let body = json!({"item_code": code, "item_name": name, "item_group": group, "stock_uom": uom, "standard_rate": rate});
        async move { uid(&admin.insert("Item", body).await).to_string() }
    };
    let (kit, guide) = (item("KIT", "Mounting Kit", "20").await, item("GUIDE", "Self-fit Guide", "5").await);
    refused(
        anna.k.insert(&anna.ctx, "ProductRule", obj(json!({"item": widget, "rule": "Requires", "other_item": service, "description": "x"}))).await,
        "not permitted",
    );
    refused(
        maria.k.insert(&maria.ctx, "ProductRule", obj(json!({"item": widget, "rule": "Requires", "other_item": widget, "description": "x"}))).await,
        "two different items",
    );
    maria.insert("ProductRule", json!({"item": widget, "rule": "Requires", "other_item": service, "description": "Widgets are installed"})).await;
    maria
        .insert(
            "ProductRule",
            json!({"item": widget, "rule": "At least per unit", "other_item": kit, "qty_per_unit": "2", "description": "Two kits hold each widget"}),
        )
        .await;
    maria.insert("ProductRule", json!({"item": service, "rule": "Excludes", "other_item": guide, "description": "Installed or self-fitted, not both"})).await;
    let lines = |l: Value| json!({"company": cid, "party_type": "Customer", "customer": acme, "items": l});
    let quote = anna.insert("Quotation", lines(json!([{"item": widget, "qty": "3"}]))).await;
    let issues = quote["rule_issues"].as_str().unwrap_or_default().to_string();
    assert!(issues.contains("Widgets are installed (Widget needs Installation)"), "{issues}");
    assert!(issues.contains("Two kits hold each widget (6 of Mounting Kit for 3 of Widget, not 0)"), "{issues}");
    let q = uid(&quote);
    refused(anna.k.submit(&anna.ctx, "Quotation", q, None).await, "product rules");
    // What the rules need can be added for you, priced like any line.
    let added = anna.action("Quotation", q, "add_rule_needs", json!({})).await;
    let said = added["message"].as_str().unwrap_or_default();
    assert!(said.starts_with("Added ") && said.contains("Mounting Kit × 6") && said.contains("Installation × 1"), "{added}");
    let quote = anna.get("Quotation", q).await;
    assert!(quote["rule_issues"].is_null(), "nothing left broken: {quote:?}");
    assert_eq!(quote["items"].as_array().map(Vec::len), Some(3));
    assert!(quote["items"].as_array().unwrap().iter().all(|l| dec(&l["rate"]) > Decimal::ZERO), "each line priced");
    let all = json!([{"item": widget, "qty": "3"}, {"item": service, "qty": "1"}, {"item": kit, "qty": "6"}, {"item": guide, "qty": "1"}]);
    let quote = anna.update("Quotation", q, lines(all)).await;
    assert_eq!(quote["rule_issues"], json!("Installed or self-fitted, not both (Installation is not quoted with Self-fit Guide)"));
    let quote = anna.update("Quotation", q, lines(json!([{"item": widget, "qty": "3"}, {"item": service, "qty": "1"}, {"item": kit, "qty": "6"}]))).await;
    assert!(quote["rule_issues"].is_null(), "nothing broken: {quote:?}");
    assert_eq!(anna.submit("Quotation", q).await["docstatus"], json!(1));

    crm_alone(&k).await;
    day_one(&k).await;
    assign_and_remind(&k).await;
    pipelines_and_quotes(&k).await;
    accounts_and_teams(&k).await;
}

/// CRM in a workspace without ERP (docs/plan/apps-ui.md §13): contacts, customers and items are the
/// platform's; quotations have no taxes and make no sales orders; nothing of ERP is there.
async fn crm_alone(k: &Arc<Kernel>) {
    let tenant = k
        .create_tenant(NewTenant {
            slug: "solo".into(),
            name: "Solo Sales".into(),
            admin_email: "admin@solo.test".into(),
            admin_password: PASSWORD.into(),
            admin_name: None,
            packages: vec!["crm".into()],
            settings: None,
        })
        .await
        .expect("tenant");
    let session = k.login("solo", "admin@solo.test", PASSWORD).await.expect("login");
    let admin = U { k: k.clone(), ctx: k.load_ctx(tenant, session.user, Some(session.token_version), "it".into()).await.expect("ctx") };
    let schema = admin.ctx.schema.clone();
    assert_eq!(schema.packages, ["core", "contacts", "parties", "products", "crm"]);
    for erp in ["SalesOrder", "Account", "TaxTemplate", "Warehouse", "FiscalYear"] {
        assert!(schema.entity(erp).is_none(), "{erp} without ERP");
    }
    let q = schema.entity("Quotation").unwrap();
    assert!(q.field("taxes").is_none() && q.field("tax_template").is_none());
    assert!(q.def.actions.iter().all(|a| a.name != "make_sales_order"));
    assert!(q.def.form.iter().all(|s| !s.fields.iter().any(|f| f == "taxes")), "the layout leaves ERP's fields out");
    let contact = schema.entity("Contact").unwrap();
    for link in ["customer", "supplier", "lead", "prospect"] {
        assert!(contact.field(link).is_some(), "Contact.{link}");
    }
    assert!(schema.entity("Item").unwrap().field("is_stock_item").is_none(), "an item's stock settings come with ERP");

    // A company gets no chart of accounts; customers, items, contacts and addresses work as before.
    let inr = admin.one("Currency", json!({"code": "INR"})).await;
    let company = admin.insert("Company", json!({"company_name": "Solo Sales", "abbr": "SS", "default_currency": inr["id"]})).await;
    let customer = admin.insert("Customer", json!({"customer_name": "Acme Buyers"})).await;
    let nos = admin.one("Uom", json!({"uom_name": "Nos"})).await;
    let group = admin.one("ItemGroup", json!({"group_name": "Products"})).await;
    let item = admin
        .insert("Item", json!({"item_code": "WIDGET", "item_name": "Widget", "item_group": group["id"], "stock_uom": nos["id"], "standard_rate": "250"}))
        .await;
    let ct =
        admin.insert("Contact", json!({"first_name": "Ann", "last_name": "Buyer", "customer": customer["id"], "emails": [{"email": "ann@acme.test"}]})).await;
    assert_eq!((ct["full_name"].as_str(), ct["email"].as_str()), (Some("Ann Buyer"), Some("ann@acme.test")));
    let addr = admin
        .insert("Address", json!({"address_line1": "1 Market St", "city": "Pune", "postal_code": "411001", "customer": customer["id"], "is_primary": true}))
        .await;
    assert_eq!(addr["address_title"].as_str(), Some("Acme Buyers"), "titled after the customer");
    assert_eq!(admin.get("Customer", uid(&customer)).await["billing_address"].as_str(), Some("1 Market St\nPune 411001"));
    refused(k.insert(&admin.ctx, "Address", obj(json!({"address_line1": "Nowhere", "city": "Pune"}))).await, "Link the address");

    // A quotation is priced without taxes, submitted, and lost.
    let quote = admin
        .insert(
            "Quotation",
            json!({"company": company["id"], "party_type": "Customer", "customer": customer["id"], "items": [{"item": item["id"], "qty": "4"}]}),
        )
        .await;
    assert_eq!((dec(&quote["net_total"]), dec(&quote["total_taxes"]), dec(&quote["grand_total"])), (n("1000"), n("0"), n("1000")));
    let quote = admin.submit("Quotation", uid(&quote)).await;
    assert_eq!(quote["status"].as_str(), Some("Open"));
    let lost = admin.insert("LostReason", json!({"reason": "Went elsewhere"})).await;
    admin.action("Quotation", uid(&quote), "mark_lost", json!({"lost_reason": lost["id"]})).await;
    assert_eq!(admin.get("Quotation", uid(&quote)).await["status"].as_str(), Some("Lost"));
    refused(k.run_action(&admin.ctx, "Quotation", uid(&quote), "make_sales_order", json!({})).await, "not found");

    // A sales rep reads customers, and converting a lead makes one, without ERP too.
    let id = k.create_user(&admin.ctx, "sol@solo.test", "Sol Rep", Some(PASSWORD), &["Sales User".to_string()], false).await.expect("user");
    let sol = U { ctx: k.load_ctx(tenant, id, None, "it".into()).await.expect("ctx"), k: k.clone() };
    assert_eq!(sol.list("Customer", json!({})).await.len(), 1, "the customers in CRM's menu");
    let lead = sol.insert("Lead", json!({"first_name": "Lia", "organization_name": "Lia Studio", "company": company["id"]})).await;
    let made = sol.action("Lead", uid(&lead), "convert_to_customer", json!({})).await;
    assert_eq!(sol.get("Customer", vid(&made)).await["customer_name"], json!("Lia Studio"));
}

/// After a refused undo nothing moved: the lead is still converted onto the customer, its deal and
/// quotation are still the customer's, and the customer, contact and address the conversion made stand.
async fn unmoved(admin: &U, lead: Uuid, made: &Value, deal: Uuid, quotation: Uuid) {
    let customer = made["id"].clone();
    let l = admin.get("Lead", lead).await;
    assert_eq!((l["status"].clone(), l["customer"].clone(), l["contact"].clone()), (json!("Converted"), customer.clone(), made["contact"].clone()), "{l:?}");
    assert!(l["conversion"].is_object(), "still knows what it made");
    assert_eq!(admin.get("Opportunity", deal).await["customer"], customer, "its deal");
    assert_eq!(admin.get("Quotation", quotation).await["customer"], customer, "its quotation");
    for (entity, rid) in [("Customer", &customer), ("Contact", &made["contact"]), ("Address", &made["address"])] {
        admin.get(entity, vid(&json!({"id": rid}))).await;
    }
}

/// A sales team's first days after switching CRMs: the same person entered again, enquiries from
/// forms and mail, a conversion onto what exists and one taken back, a deal reopened, a prospect's
/// deal, and what an erasure takes.
async fn day_one(k: &Arc<Kernel>) {
    let tenant = k
        .create_tenant(NewTenant {
            slug: "dayone".into(),
            name: "Day One".into(),
            admin_email: "admin@day.test".into(),
            admin_password: PASSWORD.into(),
            admin_name: None,
            packages: vec!["crm".into(), "crm_erp".into()],
            settings: Some(json!({"fiscal_year_start_month": 4, "timezone": *ZONE})),
        })
        .await
        .expect("tenant");
    let session = k.login("dayone", "admin@day.test", PASSWORD).await.expect("login");
    let admin = U { k: k.clone(), ctx: k.load_ctx(tenant, session.user, Some(session.token_version), "it".into()).await.expect("ctx") };
    let inr = admin.one("Currency", json!({"code": "INR"})).await;
    let company = admin.insert("Company", json!({"company_name": "Day One", "abbr": "DO", "default_currency": inr["id"]})).await;
    let cid = uid(&company).to_string();
    let group = admin.one("ItemGroup", json!({"group_name": "Products"})).await;
    let nos = admin.one("Uom", json!({"uom_name": "Nos"})).await;
    let widget = uid(&admin
        .insert("Item", json!({"item_code": "W", "item_name": "Widget", "item_group": group["id"], "stock_uom": nos["id"], "standard_rate": "100"}))
        .await)
    .to_string();
    let user = |email: &'static str, roles: &'static [&'static str]| {
        let (k, admin_ctx) = (k.clone(), admin.ctx.clone());
        async move {
            let roles: Vec<String> = roles.iter().map(|r| r.to_string()).collect();
            let id = k.create_user(&admin_ctx, email, email, Some(PASSWORD), &roles, false).await.expect("user");
            U { ctx: k.load_ctx(tenant, id, None, "it".into()).await.expect("user ctx"), k }
        }
    };
    let anna = user("anna@day.test", &["Sales User"]).await;
    let maria = user("maria@day.test", &["Sales Manager"]).await;
    let stage = |name: &'static str| {
        let admin = &admin;
        async move { admin.one("SalesStage", json!({"stage_name": name})).await["id"].clone() }
    };
    let price = admin.one("LostReason", json!({"reason": "Price"})).await["id"].clone();

    // ───────────── the same person, entered again ─────────────
    let asha = anna.insert("Lead", json!({"first_name": "Asha", "last_name": "Rao", "email": "asha@rao.test", "mobile": "+91 98450 00001"})).await;
    // The same phone written three ways, in either field, and the email in capitals.
    for again in [
        json!({"first_name": "A", "phone": "+91-98450-00001"}),
        json!({"first_name": "A", "mobile": "(+91) 9845000001"}),
        json!({"first_name": "A", "phone": "+91 98450 000 01"}),
        json!({"first_name": "A", "email": "Asha@Rao.TEST"}),
        // A trunk zero, an international 00, no country code at all: the national part.
        json!({"first_name": "A", "phone": "098450 00001"}),
        json!({"first_name": "A", "mobile": "0091 9845000001"}),
        json!({"first_name": "A", "phone": "9845000001"}),
    ] {
        let r = anna.k.insert(&anna.ctx, "Lead", obj(again.clone())).await;
        refused(r, "already exists");
    }
    let said = anna.k.insert(&anna.ctx, "Lead", obj(json!({"first_name": "A", "mobile": "98450-00001 ", "phone": "+91 98450 00001"}))).await;
    assert!(
        matches!(&said, Err(Error::Validation(e)) if e.iter().any(|f| f.message.contains(asha["number"].as_str().unwrap()) && f.message.contains("Asha Rao"))),
        "names the lead: {said:?}"
    );
    // A seven-digit number matches only itself; numbers of another length that end alike do not.
    let short = anna.insert("Lead", json!({"first_name": "Shop", "phone": "555 0123"})).await;
    refused(anna.k.insert(&anna.ctx, "Lead", obj(json!({"first_name": "S", "mobile": "555-0123"}))).await, "already exists");
    assert!(anna.insert("Lead", json!({"first_name": "Far", "phone": "+1 415 555 0123"})).await["duplicate_of"].is_null(), "not {short:?}");
    let sf = anna.insert("Lead", json!({"first_name": "Sam", "phone": "+1 415 555 0100"})).await;
    assert!(anna.insert("Lead", json!({"first_name": "Lon", "phone": "+44 415 555 0100 9"})).await["duplicate_of"].is_null(), "not {sf:?}");
    // Blank values never match.
    for _ in 0..2 {
        assert!(anna.insert("Lead", json!({"first_name": "Walk-in"})).await["duplicate_of"].is_null());
    }
    // Two people sharing one mailbox: kept apart on purpose, linked to merge or not.
    let ravi = anna.insert("Lead", json!({"first_name": "Ravi", "last_name": "Rao", "email": "asha@rao.test", "allow_duplicate": true})).await;
    assert_eq!((ravi["duplicate_of"].clone(), ravi["allow_duplicate"].clone()), (asha["id"].clone(), json!(false)));
    // Editing a lead that others match leaves it be.
    anna.update("Lead", uid(&asha), json!({"next_follow_up": today().to_string()})).await;
    // A lost lead returning is a new enquiry.
    let old = anna.insert("Lead", json!({"first_name": "Omar", "email": "omar@old.test"})).await;
    anna.update("Lead", uid(&old), json!({"status": "Lost", "lost_reason": price})).await;
    let back = anna.insert("Lead", json!({"first_name": "Omar", "email": "omar@old.test"})).await;
    assert_eq!((back["status"].clone(), back["duplicate_of"].clone()), (json!("New"), old["id"].clone()), "saved, linked to the lost one");
    // An enquiry from a form or a mail (the engine's context) is never dropped.
    let engine = Ctx::system(tenant, admin.ctx.schema.clone());
    let form =
        k.insert(&engine, "Lead", obj(json!({"first_name": "Asha", "email": "asha@rao.test", "company": cid, "source": "Website"}))).await.expect("kept");
    assert_eq!(form["duplicate_of"], asha["id"], "linked to the open lead it repeats");
    // A double submit still makes one lead.
    let twin = json!({"first_name": "Tia", "email": "tia@twin.test"});
    let (a, b) = tokio::join!(anna.k.insert(&anna.ctx, "Lead", obj(twin.clone())), anna.k.insert(&anna.ctx, "Lead", obj(twin.clone())));
    assert_eq!([a.is_ok(), b.is_ok()].iter().filter(|ok| **ok).count(), 1, "{a:?} {b:?}");
    assert_eq!(anna.list("Lead", json!({"email": "tia@twin.test"})).await.len(), 1);
    // A lead from before this release gets its phone keys from the release's backfill.
    let mut tx = k.db.system_tx().await.unwrap();
    sqlx::query("update e_lead set phone_key = null, mobile_key = null where id = $1").bind(uid(&asha)).execute(&mut *tx).await.unwrap();
    sqlx::raw_sql(include_str!("../packages/crm/sql/002_lead_keys.sql")).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(admin.get("Lead", uid(&asha)).await["mobile_key"], json!("9845000001"));
    refused(anna.k.insert(&anna.ctx, "Lead", obj(json!({"first_name": "A", "phone": "0091 98450 00001"}))).await, "already exists");

    // ───────────── converting onto what exists ─────────────
    let rao = admin.insert("Customer", json!({"customer_name": "Rao Works"})).await;
    let card = admin.insert("Contact", json!({"first_name": "Asha", "last_name": "Rao", "customer": rao["id"], "emails": [{"email": "asha@rao.test"}]})).await;
    let deal = vid(&anna.action("Lead", uid(&asha), "make_opportunity", json!({})).await);
    anna.update("Lead", uid(&asha), json!({"need": "Faster delivery", "notes": "Met at the expo; wants 50 units"})).await;
    let r = anna.k.run_action(&anna.ctx, "Lead", uid(&asha), "convert_to_customer", json!({})).await;
    refused(r, "contact Asha Rao already has");
    let done = anna.action("Lead", uid(&asha), "convert_to_customer", json!({"contact": card["id"]})).await;
    assert_eq!((done["id"].clone(), done["contact"].clone()), (rao["id"].clone(), card["id"].clone()), "the contact's customer, and the contact itself");
    assert_eq!(admin.list("Contact", json!({"customer": rao["id"]})).await.len(), 1, "no second contact");
    assert_eq!(anna.get("Opportunity", deal).await["customer"], rao["id"]);
    let notes = anna.list("Activity", json!({"opportunity": deal.to_string(), "activity_type": "Note"})).await;
    let note = notes.iter().find(|n| n["subject"].as_str().is_some_and(|s| s.starts_with("Notes from lead"))).expect("the lead's notes on the deal's story");
    assert!(note["details"].as_str().is_some_and(|d| d.contains("Need: Faster delivery") && d.contains("wants 50 units")), "{note:?}");
    assert_eq!(note["customer"], rao["id"]);
    // Refused on a customer with the same phone, then allowed when asked to make a new one.
    admin.insert("Customer", json!({"customer_name": "Chen Old", "phone": "+65 6123 4567"})).await;
    let chen = anna.insert("Lead", json!({"first_name": "Mei", "last_name": "Chen", "organization_name": "Chen New", "phone": "+65-6123-4567"})).await;
    refused(anna.k.run_action(&anna.ctx, "Lead", uid(&chen), "convert_to_customer", json!({})).await, "Create New Anyway");
    let made = anna.action("Lead", uid(&chen), "convert_to_customer", json!({"create_new": true})).await;
    assert_eq!(anna.get("Customer", vid(&made)).await["customer_name"], json!("Chen New"));

    // ───────────── undoing a conversion ─────────────
    let eve = anna
        .insert("Lead", json!({"first_name": "Eve", "organization_name": "Eve Labs", "email": "eve@labs.test", "address_line1": "1 Lab Road", "city": "Pune", "notes": "Call after 5"}))
        .await;
    let eve_deal = vid(&anna.action("Lead", uid(&eve), "make_opportunity", json!({})).await);
    let eve_call = anna.insert("Activity", json!({"activity_type": "Call", "subject": "Intro", "lead": eve["id"]})).await;
    anna.update("Opportunity", eve_deal, json!({"items": [{"item": widget, "qty": "1", "rate": "100"}]})).await;
    let eve_q = vid(&anna.action("Opportunity", eve_deal, "make_quotation", json!({})).await);
    let made = anna.action("Lead", uid(&eve), "convert_to_customer", json!({})).await;
    let eve_co = vid(&made);
    // Another lead converted onto that customer stands in the way, until it is undone itself.
    let gus = anna.insert("Lead", json!({"first_name": "Gus", "email": "gus@labs.test"})).await;
    let joined = anna.action("Lead", uid(&gus), "convert_to_customer", json!({"customer": eve_co.to_string()})).await;
    let r = anna.k.run_action(&anna.ctx, "Lead", uid(&eve), "undo_conversion", json!({})).await;
    assert!(matches!(&r, Err(Error::Invalid(m)) if m.contains("the customer Eve Labs") && m.contains("Gus") && m.contains("nothing was undone")), "{r:?}");
    unmoved(&admin, uid(&eve), &made, eve_deal, eve_q).await;
    // The customer existed before Gus's conversion: it stays, and the contact made for him goes.
    anna.action("Lead", uid(&gus), "undo_conversion", json!({})).await;
    let gus_now = anna.get("Lead", uid(&gus)).await;
    assert_eq!((gus_now["status"].clone(), gus_now["customer"].clone()), (json!("New"), Value::Null));
    refused(admin.k.get(&admin.ctx, "Contact", vid(&json!({"id": joined["contact"]})), false).await, "not found");
    admin.get("Customer", eve_co).await;
    refused(anna.k.run_action(&anna.ctx, "Lead", uid(&gus), "undo_conversion", json!({})).await, "not available");
    // Eve's undone whole: her deal, call and status back, and what was made for her gone.
    let undone = anna.action("Lead", uid(&eve), "undo_conversion", json!({})).await;
    assert_eq!(undone["removed"].as_array().map(Vec::len), Some(4), "note, address, contact, customer: {undone}");
    let eve_now = anna.get("Lead", uid(&eve)).await;
    assert_eq!((eve_now["status"].clone(), eve_now["customer"].clone(), eve_now["contact"].clone()), (json!("Qualified"), Value::Null, Value::Null));
    assert!(anna.get("Opportunity", eve_deal).await["customer"].is_null() && anna.get("Activity", uid(&eve_call)).await["customer"].is_null());
    assert_eq!(
        (anna.get("Quotation", eve_q).await["customer"].clone(), anna.get("Quotation", eve_q).await["party_type"].clone()),
        (Value::Null, json!("Lead"))
    );
    for (entity, rid) in [("Customer", made["id"].clone()), ("Contact", made["contact"].clone()), ("Address", made["address"].clone())] {
        refused(admin.k.get(&admin.ctx, entity, vid(&json!({"id": rid})), false).await, "not found");
    }
    assert_eq!(admin.list("Activity", json!({"lead": eve["id"], "activity_type": "Note"})).await.len(), 0, "the note it wrote goes too");
    refused(anna.k.run_action(&anna.ctx, "Lead", uid(&eve), "undo_conversion", json!({})).await, "not available");
    // Converted again, and ordered: undoing waits for the order to go.
    let again = anna.action("Lead", uid(&eve), "convert_to_customer", json!({})).await;
    anna.update("Opportunity", eve_deal, json!({"items": [{"item": widget, "qty": "2", "rate": "100"}]})).await;
    let q = vid(&anna.action("Opportunity", eve_deal, "make_quotation", json!({})).await);
    anna.submit("Quotation", q).await;
    let order = vid(&anna.action("Quotation", q, "make_sales_order", json!({})).await);
    anna.submit("SalesOrder", order).await;
    refused(anna.k.run_action(&anna.ctx, "Lead", uid(&eve), "undo_conversion", json!({})).await, "cancel its sales orders");
    unmoved(&admin, uid(&eve), &again, eve_deal, q).await;
    // Its deal is won by the order, and reopening it waits for the order too.
    assert_eq!(anna.get("Opportunity", eve_deal).await["status"], json!("Won"));
    refused(anna.k.run_action(&anna.ctx, "Opportunity", eve_deal, "reopen", json!({})).await, "cancel its sales order");
    anna.cancel("SalesOrder", order).await;
    assert_eq!(anna.get("Opportunity", eve_deal).await["status"], json!("Open"), "cancelling the order reopens it");
    refused(anna.k.run_action(&anna.ctx, "Lead", uid(&eve), "undo_conversion", json!({})).await, "sales order");
    unmoved(&admin, uid(&eve), &again, eve_deal, q).await;
    // A lead converted before 0.8.0 kept no record of what was made: its records come back, the customer stays.
    let fay = anna.insert("Lead", json!({"first_name": "Fay", "email": "fay@old.test"})).await;
    let fays = anna.action("Lead", uid(&fay), "convert_to_customer", json!({})).await;
    let mut tx = k.db.tenant_tx(tenant).await.unwrap();
    sqlx::query("update e_lead set conversion = null where tenant_id = $1 and id = $2").bind(tenant).bind(uid(&fay)).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    anna.action("Lead", uid(&fay), "undo_conversion", json!({})).await;
    assert_eq!(anna.get("Lead", uid(&fay)).await["status"], json!("Qualified"));
    admin.get("Customer", vid(&fays)).await;

    // ───────────── reopening a closed deal ─────────────
    let won = uid(&anna
        .insert("Opportunity", json!({"opportunity_from": "Lead", "lead": back["id"], "sales_stage": stage("Proposal").await, "amount": "500"}))
        .await);
    refused(anna.k.run_action(&anna.ctx, "Opportunity", won, "reopen", json!({})).await, "not available");
    anna.action("Opportunity", won, "mark_won", json!({})).await;
    anna.action("Opportunity", won, "reopen", json!({})).await;
    let opp = anna.get("Opportunity", won).await;
    assert_eq!((opp["status"].clone(), opp["sales_stage"].clone(), opp["closed_on"].clone()), (json!("Open"), stage("Proposal").await, Value::Null));
    assert_eq!(dec(&opp["probability"]), n("60"), "the stage's probability again");
    anna.update("Opportunity", won, json!({"sales_stage": stage("Negotiation").await})).await;
    anna.action("Opportunity", won, "mark_lost", json!({"lost_reason": "Price", "lost_detail": "Went local"})).await;
    anna.action("Opportunity", won, "reopen", json!({})).await;
    let opp = anna.get("Opportunity", won).await;
    assert_eq!((opp["sales_stage"].clone(), opp["lost_reason"].clone(), opp["lost_detail"].clone()), (stage("Negotiation").await, Value::Null, Value::Null));
    let story = anna.list("Activity", json!({"opportunity": won.to_string(), "subject": "Reopened"})).await;
    assert_eq!(story.len(), 2);
    assert!(
        story.iter().any(|a| a["details"].as_str().is_some_and(|d| d.contains("It was Lost") && d.contains("Price") && d.contains("Went local"))),
        "{story:?}"
    );

    // A deal open before this release reopens where it stood: the release's backfill.
    let older = uid(&anna.insert("Opportunity", json!({"opportunity_from": "Lead", "lead": back["id"], "sales_stage": stage("Proposal").await})).await);
    let mut tx = k.db.system_tx().await.unwrap();
    sqlx::query("update e_opportunity set open_stage = null where id = $1").bind(older).execute(&mut *tx).await.unwrap();
    sqlx::raw_sql(include_str!("../packages/crm/sql/001_open_stage.sql")).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    anna.action("Opportunity", older, "mark_won", json!({})).await;
    anna.action("Opportunity", older, "reopen", json!({})).await;
    assert_eq!(anna.get("Opportunity", older).await["sales_stage"], stage("Proposal").await);

    // ───────────── a prospect's deal ─────────────
    let initech = maria.insert("Prospect", json!({"prospect_name": "Initech Labs"})).await;
    let pdeal = maria.insert("Opportunity", json!({"opportunity_from": "Prospect", "prospect": initech["id"], "amount": "900"})).await;
    assert_eq!((pdeal["party_name"].clone(), pdeal["customer"].clone()), (json!("Initech Labs"), Value::Null), "a prospect with no leads");
    refused(maria.k.insert(&maria.ctx, "Opportunity", obj(json!({"opportunity_from": "Prospect"}))).await, "prospect");
    maria.update("Opportunity", uid(&pdeal), json!({"items": [{"item": widget, "qty": "9", "rate": "100"}]})).await;
    let pq = vid(&maria.action("Opportunity", uid(&pdeal), "make_quotation", json!({})).await);
    let quote = maria.get("Quotation", pq).await;
    assert_eq!(
        (quote["party_type"].clone(), quote["prospect"].clone(), quote["party_name"].clone()),
        (json!("Prospect"), initech["id"].clone(), json!("Initech Labs"))
    );
    maria.submit("Quotation", pq).await;
    refused(maria.k.run_action(&maria.ctx, "Quotation", pq, "make_sales_order", json!({})).await, "not a customer yet");
    let pat = maria.insert("Lead", json!({"first_name": "Pat", "prospect": initech["id"]})).await;
    let became = maria.action("Lead", uid(&pat), "convert_to_customer", json!({})).await;
    assert_eq!(maria.get("Prospect", uid(&initech)).await["customer"], became["id"]);
    assert_eq!(maria.get("Opportunity", uid(&pdeal)).await["customer"], became["id"], "its deal moves to the customer");
    assert_eq!(maria.get("Quotation", pq).await["customer"], became["id"], "so does its quotation");
    let so = vid(&maria.action("Quotation", pq, "make_sales_order", json!({})).await);
    assert_eq!(maria.get("SalesOrder", so).await["customer"], became["id"]);

    // ───────────── what an erasure takes ─────────────
    let kim = anna
        .insert(
            "Lead",
            json!({"first_name": "Kim", "organization_name": "Kim Consulting", "email": "kim@private.test", "address_line1": "4 Quiet Lane", "postal_code": "411001",
                   "city": "Pune", "need": "A new website", "notes": "Her husband is the other director"}),
        )
        .await;
    let met =
        anna.insert("Activity", json!({"activity_type": "Meeting", "subject": "Lunch with Kim", "details": "Talked about her move", "lead": kim["id"]})).await;
    let solo = maria
        .insert("Prospect", json!({"prospect_name": "Kim Consulting", "email": "kim@private.test", "phone": "+91 99999 00000", "notes": "Sole trader"}))
        .await;
    k.erase_personal_data(&admin.ctx, "kim@private.test", "Request 9").await.expect("erased");
    let kim = admin.get("Lead", uid(&kim)).await;
    for f in ["organization_name", "address_line1", "postal_code", "need", "notes", "email"] {
        assert!(kim[f].is_null(), "Lead.{f}: {kim:?}");
    }
    let met = admin.get("Activity", uid(&met)).await;
    assert!(met["subject"].as_str().is_some_and(|s| s.starts_with("Erased ")) && met["details"].is_null(), "{met:?}");
    let solo = admin.get("Prospect", uid(&solo)).await;
    assert!(solo["email"].is_null() && solo["phone"].is_null() && solo["notes"].is_null() && solo["prospect_name"] == json!("Kim Consulting"), "{solo:?}");
}

/// Who was told of `kind` about `record`, in order.
async fn told(k: &Kernel, tenant: Uuid, kind: &str, record: Uuid) -> Vec<String> {
    let mut tx = k.db.system_tx().await.unwrap();
    let who: Vec<String> = sqlx::query_scalar(
        "select u.email::text from notification n join app_user u on u.tenant_id = n.tenant_id and u.id = n.user_id \
         where n.tenant_id = $1 and n.kind = $2 and n.record_id = $3 order by n.created_at, n.id",
    )
    .bind(tenant)
    .bind(kind)
    .bind(record)
    .fetch_all(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    who
}

/// Runs SQL as the engine (backdating, a release's backfill).
async fn sql(k: &Kernel, q: &str, record: Uuid) {
    let mut tx = k.db.system_tx().await.unwrap();
    sqlx::query(sqlx::AssertSqlSafe(q.to_string())).bind(record).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
}

async fn remind(k: &Kernel) {
    ferrum_app_crm::maintenance(k).await.expect("maintenance");
}

/// A sales team's leads shared out by rules, the reminders that keep work from slipping, and the
/// reports a manager reads at the end of the month.
async fn assign_and_remind(k: &Arc<Kernel>) {
    let tenant = k
        .create_tenant(NewTenant {
            slug: "remind".into(),
            name: "Remind".into(),
            admin_email: "admin@rem.test".into(),
            admin_password: PASSWORD.into(),
            admin_name: None,
            packages: vec!["crm".into(), "crm_erp".into()],
            settings: Some(json!({"fiscal_year_start_month": 4, "timezone": *ZONE})),
        })
        .await
        .expect("tenant");
    let session = k.login("remind", "admin@rem.test", PASSWORD).await.expect("login");
    let admin = U { k: k.clone(), ctx: k.load_ctx(tenant, session.user, Some(session.token_version), "it".into()).await.expect("ctx") };
    let inr = admin.one("Currency", json!({"code": "INR"})).await;
    let company = admin.insert("Company", json!({"company_name": "Remind", "abbr": "RM", "default_currency": inr["id"]})).await;
    let cid = uid(&company).to_string();
    let user = |email: &'static str, roles: &'static [&'static str]| {
        let (k, admin_ctx) = (k.clone(), admin.ctx.clone());
        async move {
            let roles: Vec<String> = roles.iter().map(|r| r.to_string()).collect();
            let id = k.create_user(&admin_ctx, email, email, Some(PASSWORD), &roles, false).await.expect("user");
            U { ctx: k.load_ctx(tenant, id, None, "it".into()).await.expect("user ctx"), k }
        }
    };
    let anna = user("anna@rem.test", &["Sales User"]).await;
    let ben = user("ben@rem.test", &["Sales User"]).await;
    let cara = user("cara@rem.test", &["Sales User"]).await;
    let dave = user("dave@rem.test", &["Sales User"]).await;
    let maria = user("maria@rem.test", &["Sales Manager"]).await;
    let deactivate = |who: &U| {
        let (k, email) = (k.clone(), who.ctx.email.clone());
        async move {
            let mut tx = k.db.system_tx().await.unwrap();
            sqlx::query("update app_user set is_active = false where tenant_id = $1 and email = $2").bind(tenant).bind(email).execute(&mut *tx).await.unwrap();
            tx.commit().await.unwrap();
        }
    };
    let engine = Ctx::system(tenant, admin.ctx.schema.clone());
    let arrive = |v: Value| {
        let (k, engine, cid) = (k.clone(), engine.clone(), cid.clone());
        async move {
            let mut v = obj(v);
            v.insert("company".into(), json!(cid));
            k.insert(&engine, "Lead", v).await.expect("a lead arrives")
        }
    };
    let owner = |d: &Doc| d["assigned_to"].as_str().unwrap_or_default().to_string();

    // ───────────── assignment rules ─────────────
    let all = admin.one("Territory", json!({"territory_name": "All Territories"})).await;
    let india = maria.insert("Territory", json!({"territory_name": "India", "parent_territory": all["id"], "is_group": true})).await;
    let pune = maria.insert("Territory", json!({"territory_name": "Pune", "parent_territory": india["id"]})).await;
    // Cara's lead, from before she left.
    let hers = maria.insert("Lead", json!({"first_name": "Hal", "source": "Website", "assigned_to": "cara@rem.test"})).await;
    refused(
        maria.k.insert(&maria.ctx, "LeadAssignmentRule", obj(json!({"rule_name": "x", "people": [{"user": "nobody@rem.test"}]}))).await,
        "not a user of this workspace",
    );
    refused(anna.k.insert(&anna.ctx, "LeadAssignmentRule", obj(json!({"rule_name": "mine", "people": [{"user": "anna@rem.test"}]}))).await, "not permitted");
    // Tried first, but its one person leaves: it passes leads on.
    let only_cara = maria
        .insert("LeadAssignmentRule", json!({"rule_name": "Referrals", "rule_order": 0, "source": "Referral", "people": [{"user": "cara@rem.test"}]}))
        .await;
    let web = maria
        .insert(
            "LeadAssignmentRule",
            json!({"rule_name": "Website", "rule_order": 1, "source": "Website",
                   "people": [{"user": "anna@rem.test"}, {"user": "Cara@rem.test"}, {"user": "ben@rem.test"}]}),
        )
        .await;
    let shows = maria
        .insert(
            "LeadAssignmentRule",
            json!({"rule_name": "India shows", "rule_order": 2, "territory": india["id"], "method": "Fewest Open Leads",
                   "people": [{"user": "ben@rem.test"}, {"user": "anna@rem.test"}]}),
        )
        .await;
    deactivate(&cara).await;
    assert!(only_cara["id"].is_string());

    // In turn, over three people with one inactive.
    let l1 = arrive(json!({"first_name": "One", "source": "Website"})).await;
    let l2 = arrive(json!({"first_name": "Two", "source": "Website"})).await;
    let l3 = arrive(json!({"first_name": "Three", "source": "Website"})).await;
    assert_eq!([owner(&l1), owner(&l2), owner(&l3)], ["anna@rem.test", "ben@rem.test", "anna@rem.test"], "cara is skipped");
    assert_eq!(
        (l1["assignment_rule"].clone(), maria.get("LeadAssignmentRule", uid(&web)).await["last_assigned_to"].clone()),
        (web["id"].clone(), json!("anna@rem.test"))
    );
    assert_eq!(told(k, tenant, "crm.assigned", uid(&l1)).await, ["anna@rem.test"], "the new owner is told");
    // Two arriving at the same instant take two turns.
    let (a, b) = tokio::join!(arrive(json!({"first_name": "Four", "source": "Website"})), arrive(json!({"first_name": "Five", "source": "Website"})));
    let mut two = [owner(&a), owner(&b)];
    two.sort();
    assert_eq!(two, ["anna@rem.test", "ben@rem.test"], "two people, not one twice");
    // Not a website lead: the first rule passes it on; the India rule takes Pune, to whoever holds fewer.
    let open = |who: &'static str| {
        let maria = &maria;
        async move { maria.list("Lead", json!({"assigned_to": who, "status": "New"})).await.len() }
    };
    let (na, nb) = (open("anna@rem.test").await, open("ben@rem.test").await);
    let fewer = if nb < na { "ben@rem.test" } else { "anna@rem.test" };
    let show = arrive(json!({"first_name": "Six", "source": "Trade Show", "territory": pune["id"]})).await;
    assert_eq!((owner(&show), show["assignment_rule"].clone()), (fewer.to_string(), shows["id"].clone()), "{na} vs {nb}");
    let tie_broken = arrive(json!({"first_name": "Seven", "source": "Trade Show", "territory": pune["id"]})).await;
    assert_ne!(owner(&tie_broken), "", "given");
    // Referral: its rule's one person left; nothing else takes it, so it stays as before rules.
    let referral = arrive(json!({"first_name": "Eight", "source": "Referral"})).await;
    assert_eq!((owner(&referral).as_str(), referral["assignment_rule"].clone()), ("system@ferrum.local", Value::Null));
    // A person's own lead is kept, and an owner someone chose is never overridden.
    let own = anna.insert("Lead", json!({"first_name": "Mine", "source": "Website", "assigned_to": "anna@rem.test"})).await;
    assert_eq!((owner(&own).as_str(), own["assignment_rule"].clone()), ("anna@rem.test", Value::Null));
    let chosen = maria.insert("Lead", json!({"first_name": "Yours", "source": "Website", "assigned_to": "ben@rem.test"})).await;
    assert_eq!(owner(&chosen), "ben@rem.test");
    assert_eq!(told(k, tenant, "crm.assigned", uid(&own)).await.len(), 0, "nobody tells themself");
    // A rep's own lead, typed in with no owner, is theirs: the rules here take what arrives by itself.
    assert_eq!(web["takes"], json!("Leads that arrive by themselves (forms, mail, integrations)"), "the default");
    let typed = anna.insert("Lead", json!({"first_name": "Typed", "source": "Website"})).await;
    assert_eq!((owner(&typed).as_str(), typed["assignment_rule"].clone()), ("anna@rem.test", Value::Null));
    let from_form = arrive(json!({"first_name": "Formed", "source": "Website"})).await;
    assert_eq!(from_form["assignment_rule"], web["id"], "while a form's lead goes round");
    // A rule that takes leads people enter too: an ownerless one goes round, one a rep chose stays.
    let cold = maria
        .insert(
            "LeadAssignmentRule",
            json!({"rule_name": "Cold calls", "rule_order": 3, "source": "Cold Call", "takes": "Also leads people enter or import without an owner",
                   "people": [{"user": "ben@rem.test"}]}),
        )
        .await;
    let left_empty = anna.insert("Lead", json!({"first_name": "Open", "source": "Cold Call"})).await;
    assert_eq!((owner(&left_empty).as_str(), left_empty["assignment_rule"].clone()), ("ben@rem.test", cold["id"].clone()));
    anna.get("Lead", uid(&left_empty)).await;
    let kept = anna.insert("Lead", json!({"first_name": "Kept", "source": "Cold Call", "assigned_to": "anna@rem.test"})).await;
    assert_eq!((owner(&kept).as_str(), kept["assignment_rule"].clone()), ("anna@rem.test", Value::Null));
    // An owner changed by hand is that person's choice, not the rule's.
    let moved = maria.update("Lead", uid(&l1), json!({"assigned_to": "ben@rem.test"})).await;
    assert!(moved["assignment_rule"].is_null());
    assert_eq!(told(k, tenant, "crm.assigned", uid(&l1)).await, ["anna@rem.test", "ben@rem.test"]);
    // Assign by Rules: for a lead whose owner left, not one with an owner.
    refused(maria.k.run_action(&maria.ctx, "Lead", uid(&l1), "assign_by_rules", json!({})).await, "is ben@rem.test's");
    // Asked for, by a person: every enabled rule, the default Website one included.
    let done = maria.action("Lead", uid(&hers), "assign_by_rules", json!({})).await;
    let hal = maria.get("Lead", uid(&hers)).await;
    assert_eq!((hal["assigned_to"].clone(), hal["assignment_rule"].clone()), (done["assigned_to"].clone(), web["id"].clone()));
    assert_ne!(owner(&hal), "cara@rem.test");
    assert_eq!(told(k, tenant, "crm.assigned", uid(&hers)).await, ["cara@rem.test".to_string(), owner(&hal)], "she was told while she was here");
    let mut tx = k.db.system_tx().await.unwrap();
    let story: Vec<(String, Option<Uuid>)> =
        sqlx::query_as("select action, actor from audit_event where tenant_id = $1 and entity = 'Lead' and record_id = $2 and action = 'assign_by_rules'")
            .bind(tenant)
            .bind(uid(&hers))
            .fetch_all(&mut *tx)
            .await
            .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(story, [("assign_by_rules".to_string(), Some(maria.ctx.user))], "the story says who asked");

    // ───────────── what the reminders stand on ─────────────
    let stage = |name: &'static str| {
        let admin = &admin;
        async move { admin.one("SalesStage", json!({"stage_name": name})).await["id"].clone() }
    };
    let deal = anna.insert("Opportunity", json!({"opportunity_from": "Lead", "lead": l3["id"], "amount": "1000"})).await;
    let did = uid(&deal);
    assert!(deal["stage_changed_at"].is_string() && deal["last_activity_at"].is_null());
    let since = deal["stage_changed_at"].clone();
    let kept = anna.update("Opportunity", did, json!({"amount": "1200"})).await;
    assert_eq!(kept["stage_changed_at"], since, "the stage did not move");
    let moved = anna.update("Opportunity", did, json!({"sales_stage": stage("Proposal").await})).await;
    assert_ne!(moved["stage_changed_at"], since);
    // A call logged after the fact counts when it happened; its outcome and length are kept, and the
    // next follow-up is set in the same step.
    let yesterday = (chrono::Utc::now() - chrono::Duration::days(1)).to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let call = anna.insert("Activity", json!({"activity_type": "Call", "subject": "Intro", "opportunity": did.to_string(), "due_at": yesterday})).await;
    let next = (today() + chrono::Duration::days(3)).to_string();
    anna.action("Activity", uid(&call), "complete", json!({"call_outcome": "No Answer", "duration_minutes": 2, "next_follow_up": next})).await;
    let call = anna.get("Activity", uid(&call)).await;
    assert_eq!((call["status"].clone(), call["call_outcome"].clone(), call["duration_minutes"].clone()), (json!("Done"), json!("No Answer"), json!(2)));
    let deal = anna.get("Opportunity", did).await;
    assert_eq!(deal["next_follow_up"], json!(next));
    let at = |v: &Value| ferrum_kernel::value::parse_datetime(v.as_str().unwrap()).unwrap();
    assert_eq!(at(&deal["last_activity_at"]), at(&json!(yesterday)), "when it happened");
    assert_eq!(at(&anna.get("Lead", uid(&l3)).await["last_touched_at"]), at(&json!(yesterday)), "the lead was touched too");
    // The release's backfill gives records already there the same dates.
    sql(k, "update e_opportunity set stage_changed_at = null, last_activity_at = null where id = $1", did).await;
    sql(k, "update e_lead set last_touched_at = null where id = $1", uid(&l3)).await;
    // Overdue already when the release arrives: counted as told, so nobody is flooded.
    let before = (chrono::Utc::now() - chrono::Duration::hours(5)).to_rfc3339();
    let stale = anna.insert("Activity", json!({"activity_type": "Task", "subject": "From the old CRM", "lead": l3["id"], "due_at": before})).await;
    let mut tx = k.db.system_tx().await.unwrap();
    sqlx::raw_sql(include_str!("../packages/crm/sql/003_touch_dates.sql")).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let back = admin.get("Opportunity", did).await;
    assert!((at(&back["stage_changed_at"]) - at(&moved["stage_changed_at"])).num_seconds().abs() < 5, "{back:?} {moved:?}");
    assert_eq!(at(&back["last_activity_at"]), at(&json!(yesterday)));
    assert_eq!(at(&admin.get("Lead", uid(&l3)).await["last_touched_at"]), at(&json!(yesterday)));

    // ───────────── reminders ─────────────
    // An activity overdue: told once, not again, again once moved and overdue again.
    let three_hours_ago = (chrono::Utc::now() - chrono::Duration::hours(3)).to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let task = anna.insert("Activity", json!({"activity_type": "Task", "subject": "Send brochure", "lead": l1["id"], "due_at": three_hours_ago})).await;
    remind(k).await;
    remind(k).await;
    assert_eq!(told(k, tenant, "crm.activity_overdue", uid(&task)).await, ["anna@rem.test"]);
    assert!(told(k, tenant, "crm.activity_overdue", uid(&stale)).await.is_empty(), "overdue before the release");
    anna.update("Activity", uid(&task), json!({"due_at": (chrono::Utc::now() + chrono::Duration::days(1)).to_rfc3339()})).await;
    remind(k).await;
    anna.update("Activity", uid(&task), json!({"due_at": three_hours_ago})).await;
    remind(k).await;
    assert_eq!(told(k, tenant, "crm.activity_overdue", uid(&task)).await.len(), 2, "a new occasion");
    // My work: the overdue task and the deal to follow up, each linking to its record.
    let site = k.hooks.site("crm").expect("CRM's endpoints").clone();
    let req = ferrum_kernel::SiteRequest {
        tenant,
        user: Some(anna.ctx.clone()),
        method: "GET".into(),
        path: "/work".into(),
        query: Default::default(),
        headers: Default::default(),
        body: vec![],
        client_ip: "203.0.113.7".into(),
    };
    anna.update("Lead", uid(&own), json!({"next_follow_up": today().to_string()})).await;
    let work: Value = serde_json::from_slice(&site.handle(k, req).await.expect("work").body).unwrap();
    let items = work["items"].as_array().unwrap();
    let item = |title: &str| items.iter().find(|i| i["title"] == json!(title)).unwrap_or_else(|| panic!("{title} in {work}")).clone();
    assert_eq!(item("Send brochure")["url"], json!(format!("/crm/e/Lead/{}", uid(&l1))));
    assert_eq!(item("Send brochure")["priority"], json!("High"), "overdue");
    assert_eq!(item("Mine")["due"], json!(today().to_string()));
    assert!(items.iter().all(|i| i["title"] != json!("Intro")), "done is not work");

    // A new lead nobody touched; a converted one, or one with a call planned, raises nothing.
    let quiet_lead = arrive(json!({"first_name": "Nine", "source": "Website"})).await;
    let converted = arrive(json!({"first_name": "Ten", "source": "Website"})).await;
    maria.action("Lead", uid(&converted), "convert_to_customer", json!({})).await;
    let planned = arrive(json!({"first_name": "Eleven", "source": "Website"})).await;
    maria.insert("Activity", json!({"activity_type": "Call", "subject": "First call", "lead": planned["id"]})).await;
    for l in [&quiet_lead, &converted, &planned] {
        sql(k, "update e_lead set created_at = now() - interval '25 hours' where id = $1", uid(l)).await;
    }
    remind(k).await;
    remind(k).await;
    assert_eq!(told(k, tenant, "crm.lead_untouched", uid(&quiet_lead)).await, [owner(&quiet_lead)]);
    assert!(told(k, tenant, "crm.lead_untouched", uid(&converted)).await.is_empty() && told(k, tenant, "crm.lead_untouched", uid(&planned)).await.is_empty());

    // A deal gone quiet with no next step: told of both; something done ends both; quiet again, told again.
    let gone = anna.insert("Opportunity", json!({"opportunity_from": "Lead", "lead": l3["id"], "amount": "500"})).await;
    let back20 = "update e_opportunity set created_at = now() - interval '20 days', stage_changed_at = now() - interval '20 days', \
                  last_activity_at = least(last_activity_at, now() - interval '20 days') where id = $1";
    sql(k, back20, uid(&gone)).await;
    remind(k).await;
    remind(k).await;
    assert_eq!(told(k, tenant, "crm.deal_quiet", uid(&gone)).await, ["anna@rem.test"]);
    assert_eq!(told(k, tenant, "crm.deal_no_step", uid(&gone)).await, ["anna@rem.test"]);
    assert!(told(k, tenant, "crm.deal_quiet", did).await.is_empty(), "a deal worked on lately");
    anna.insert("Activity", json!({"activity_type": "Note", "subject": "Asked for a budget", "opportunity": gone["id"]})).await;
    remind(k).await;
    sql(k, back20, uid(&gone)).await;
    sql(k, "update e_opportunity set last_activity_at = now() - interval '20 days' where id = $1", uid(&gone)).await;
    remind(k).await;
    assert_eq!(told(k, tenant, "crm.deal_quiet", uid(&gone)).await.len(), 2, "quiet again");
    assert_eq!(told(k, tenant, "crm.deal_no_step", uid(&gone)).await.len(), 2);
    anna.update("Opportunity", uid(&gone), json!({"next_step": "Call the CFO"})).await;
    remind(k).await;
    assert_eq!(told(k, tenant, "crm.deal_no_step", uid(&gone)).await.len(), 2, "it has a next step now");
    // A closed deal raises nothing.
    let won = anna.insert("Opportunity", json!({"opportunity_from": "Lead", "lead": l3["id"], "amount": "100"})).await;
    sql(k, back20, uid(&won)).await;
    anna.action("Opportunity", uid(&won), "mark_won", json!({})).await;
    remind(k).await;
    assert!(told(k, tenant, "crm.deal_quiet", uid(&won)).await.is_empty() && told(k, tenant, "crm.deal_no_step", uid(&won)).await.is_empty());
    // An owner who left is told nothing; after a handover, the new owner is.
    let daves = dave.insert("Opportunity", json!({"opportunity_from": "Lead", "lead": l2["id"], "amount": "900", "next_step": "Demo"})).await;
    sql(k, back20, uid(&daves)).await;
    deactivate(&dave).await;
    remind(k).await;
    assert!(told(k, tenant, "crm.deal_quiet", uid(&daves)).await.is_empty());
    maria.update("Opportunity", uid(&daves), json!({"assigned_to": "ben@rem.test"})).await;
    remind(k).await;
    assert_eq!(told(k, tenant, "crm.deal_quiet", uid(&daves)).await, ["ben@rem.test"]);
    ben.get("Opportunity", uid(&daves)).await;

    // Thresholds at zero switch reminders off.
    maria.insert("CrmSettings", json!({"overdue_activity_hours": 0, "untouched_lead_hours": 0, "quiet_deal_days": 0, "no_next_step_days": 0})).await;
    let late = anna.insert("Activity", json!({"activity_type": "Task", "subject": "Too late", "lead": l1["id"], "due_at": three_hours_ago})).await;
    let silent = arrive(json!({"first_name": "Twelve", "source": "Website"})).await;
    sql(k, "update e_lead set created_at = now() - interval '25 hours' where id = $1", uid(&silent)).await;
    remind(k).await;
    assert!(told(k, tenant, "crm.activity_overdue", uid(&late)).await.is_empty() && told(k, tenant, "crm.lead_untouched", uid(&silent)).await.is_empty());

    // ───────────── reports ─────────────
    // A second company keeps the reports' figures to a known set.
    let co2 = admin.insert("Company", json!({"company_name": "Reports Co", "abbr": "RC", "default_currency": inr["id"]})).await;
    let c2 = uid(&co2).to_string();
    let acme = maria.insert("Prospect", json!({"prospect_name": "Acme"})).await;
    let deal = |who: &'static str, amount: &'static str, source: &'static str, competitors: Value| {
        let (maria, c2, acme) = (&maria, c2.clone(), acme["id"].clone());
        async move {
            uid(&maria
                .insert(
                    "Opportunity",
                    json!({"opportunity_from": "Prospect", "prospect": acme, "company": c2, "amount": amount, "source": source,
                           "assigned_to": who, "competitors": competitors}),
                )
                .await)
        }
    };
    let a = deal("anna@rem.test", "1000", "Website", json!([])).await;
    let b = deal("anna@rem.test", "500", "Referral", json!([])).await;
    let c = deal("anna@rem.test", "300", "Website", json!([{"competitor": "Rival"}])).await;
    let d = deal("anna@rem.test", "200", "Referral", json!([{"competitor": "Rival"}, {"competitor": "Other"}])).await;
    let e = deal("ben@rem.test", "700", "Website", json!([])).await;
    let f = deal("ben@rem.test", "2000", "Website", json!([])).await;
    sql(k, "update e_opportunity set created_at = now() - interval '10 days' where id = $1", a).await;
    sql(k, "update e_opportunity set created_at = now() - interval '4 days' where id = $1", b).await;
    for w in [a, b, f] {
        maria.action("Opportunity", w, "mark_won", json!({})).await;
    }
    for (l, reason) in [(c, "Price"), (d, "Price"), (e, "Timing")] {
        maria.action("Opportunity", l, "mark_lost", json!({"lost_reason": reason})).await;
    }
    let span = [("company", c2.clone()), ("from_date", (today() - chrono::Duration::days(30)).to_string()), ("to_date", today().to_string())];
    let with = |extra: (&'static str, &'static str)| {
        let mut p: Vec<(&str, String)> = span.to_vec();
        p.push((extra.0, extra.1.to_string()));
        p
    };
    let rate = maria.report("win_rate", &with(("group_by", "Owner"))).await;
    let row = |r: &Value, key: &str, group: &str| {
        r["rows"].as_array().unwrap().iter().find(|x| x[key] == json!(group)).unwrap_or_else(|| panic!("{group} in {r}")).clone()
    };
    let an = row(&rate, "group", "anna@rem.test");
    assert_eq!(
        (an["won"].clone(), an["lost"].clone(), dec(&an["win_rate_pct"]), dec(&an["won_amount"]), dec(&an["lost_amount"])),
        (json!(2), json!(2), n("50"), n("1500"), n("500"))
    );
    assert_eq!((rate["totals"]["won"].clone(), rate["totals"]["lost"].clone(), dec(&rate["totals"]["win_rate_pct"])), (json!(3), json!(3), n("50")));
    let by_source = maria.report("win_rate", &with(("group_by", "Source"))).await;
    let w = row(&by_source, "group", "Website");
    assert_eq!((w["won"].clone(), w["lost"].clone(), dec(&w["win_rate_pct"])), (json!(2), json!(2), n("50")));
    let by_month = maria.report("win_rate", &with(("group_by", "Month"))).await;
    assert_eq!(by_month["rows"].as_array().map(Vec::len), Some(1), "all closed this month");
    let cycle = maria.report("sales_cycle", &with(("group_by", "Owner"))).await;
    let an = row(&cycle, "group", "anna@rem.test");
    assert_eq!((an["deals"].clone(), dec(&an["average_days"]), dec(&an["shortest_days"]), dec(&an["longest_days"])), (json!(2), n("7"), n("4"), n("10")));
    assert_eq!(dec(&row(&cycle, "group", "ben@rem.test")["average_days"]), n("0"));
    let stages = maria.report("sales_cycle", &with(("group_by", "Stage"))).await;
    assert_eq!(row(&stages, "group", "Prospecting")["deals"], json!(3), "each won deal stood in Prospecting: {stages}");
    let lost = maria.report("lost_reasons", &span).await;
    let price = row(&lost, "reason", "Price");
    assert_eq!(
        (price["deals"].clone(), dec(&price["amount"]), dec(&price["share_pct"]), price["competitors"].clone()),
        (json!(2), n("500"), n("41.67"), json!("Rival (2), Other (1)"))
    );
    assert_eq!(lost["rows"][0]["reason"], json!("Price"), "the most deals first");
    assert_eq!(dec(&row(&lost, "reason", "Timing")["amount"]), n("700"));
    // Activity by person: Rita's calls, meeting and task, not her note.
    let rita = user("rita@rem.test", &["Sales User"]).await;
    for (kind, outcome, minutes) in [("Call", Some("Reached"), 5), ("Call", Some("No Answer"), 2), ("Meeting", None, 30), ("Task", None, 0), ("Note", None, 0)]
    {
        let a = rita.insert("Activity", json!({"activity_type": kind, "subject": kind})).await;
        if kind != "Note" {
            rita.action("Activity", uid(&a), "complete", json!({"call_outcome": outcome, "duration_minutes": minutes})).await;
        }
    }
    let busy = maria.report("activity_by_person", &with(("period", "Month"))).await;
    let r = row(&busy, "person", "rita@rem.test");
    assert_eq!(
        [&r["calls"], &r["reached"], &r["call_minutes"], &r["meetings"], &r["tasks"], &r["emails"], &r["total"]].map(|v| v.as_i64().unwrap()),
        [2, 1, 7, 1, 1, 0, 4]
    );
    assert!(maria.report("activity_by_person", &with(("period", "Week"))).await["rows"].as_array().is_some_and(|r| !r.is_empty()));
}

/// A team arriving with two pipelines (new business and renewals): the upgrade's migration on data
/// made the old way, a stage name in both, deals moved across, the board's and the reports' data per
/// pipeline, what a stage asks of a deal entering it, contacts with buying roles, lines priced from a
/// price list in another currency, a quotation sent, rejected, revised twice and ordered, and the
/// forecast by owner and category on known figures.
async fn pipelines_and_quotes(k: &Arc<Kernel>) {
    let tenant = k
        .create_tenant(NewTenant {
            slug: "pipes".into(),
            name: "Pipes".into(),
            admin_email: "admin@pipes.test".into(),
            admin_password: PASSWORD.into(),
            admin_name: None,
            packages: vec!["crm".into(), "crm_erp".into()],
            settings: Some(json!({"fiscal_year_start_month": 4, "timezone": *ZONE})),
        })
        .await
        .expect("tenant");
    let session = k.login("pipes", "admin@pipes.test", PASSWORD).await.expect("login");
    let admin = U { k: k.clone(), ctx: k.load_ctx(tenant, session.user, Some(session.token_version), "it".into()).await.expect("ctx") };
    let inr = admin.one("Currency", json!({"code": "INR"})).await;
    let usd = admin.one("Currency", json!({"code": "USD"})).await;
    let company = admin.insert("Company", json!({"company_name": "Pipes", "abbr": "PP", "default_currency": inr["id"]})).await;
    let cid = uid(&company).to_string();
    let group = admin.one("ItemGroup", json!({"group_name": "Products"})).await;
    let nos = admin.one("Uom", json!({"uom_name": "Nos"})).await;
    let pack = admin.insert("Uom", json!({"uom_name": "Pack"})).await;
    let widget = uid(&admin
        .insert(
            "Item",
            json!({"item_code": "W", "item_name": "Widget", "item_group": group["id"], "stock_uom": nos["id"], "standard_rate": "100",
                   "uoms": [{"uom": pack["id"], "conversion_factor": "10"}]}),
        )
        .await)
    .to_string();
    let gadget = uid(&admin
        .insert("Item", json!({"item_code": "G", "item_name": "Gadget", "item_group": group["id"], "stock_uom": nos["id"], "standard_rate": "40"}))
        .await)
    .to_string();
    let user = |email: &'static str, roles: &'static [&'static str]| {
        let (k, admin_ctx) = (k.clone(), admin.ctx.clone());
        async move {
            let roles: Vec<String> = roles.iter().map(|r| r.to_string()).collect();
            let id = k.create_user(&admin_ctx, email, email, Some(PASSWORD), &roles, false).await.expect("user");
            U { ctx: k.load_ctx(tenant, id, None, "it".into()).await.expect("user ctx"), k }
        }
    };
    let anna = user("anna@pipes.test", &["Sales User"]).await;
    let maria = user("maria@pipes.test", &["Sales Manager"]).await;
    user("ben@pipes.test", &["Sales User"]).await;
    let acme = admin.insert("Customer", json!({"customer_name": "Acme"})).await["id"].clone();
    let idv = |v: &Value| -> Uuid { v.as_str().unwrap().parse().unwrap() };
    let deal = |title: &str, extra: Value| {
        let mut v = json!({"opportunity_from": "Customer", "customer": acme.clone(), "title": title});
        v.as_object_mut().unwrap().extend(extra.as_object().cloned().unwrap());
        v
    };

    // ───────────── one pipeline out of the box, and the upgrade's migration ─────────────
    let sales = admin.one("SalesPipeline", json!({})).await;
    assert_eq!((sales["pipeline_name"].clone(), sales["is_default"].clone()), (json!("Sales Pipeline"), json!(true)));
    let stages = admin.list("SalesStage", json!({"pipeline": sales["id"]})).await;
    assert_eq!(stages.len(), 7, "the seeded stages are its");
    let stage = |pipeline: &Value, name: &'static str| {
        let (admin, pipeline) = (&admin, pipeline.clone());
        async move { admin.one("SalesStage", json!({"pipeline": pipeline, "stage_name": name})).await["id"].clone() }
    };
    let proposal = stage(&sales["id"], "Proposal").await;
    let negotiation = stage(&sales["id"], "Negotiation").await;
    // Deals made as 0.9.0 made them: an open one, a won one and a lost one, each remembering its last open stage.
    let open = uid(&anna.insert("Opportunity", deal("Open one", json!({"sales_stage": proposal, "amount": "100"}))).await);
    let won = uid(&anna.insert("Opportunity", deal("Won one", json!({"sales_stage": negotiation, "amount": "200"}))).await);
    anna.action("Opportunity", won, "mark_won", json!({})).await;
    let lost = uid(&anna.insert("Opportunity", deal("Lost one", json!({"sales_stage": proposal}))).await);
    anna.action("Opportunity", lost, "mark_lost", json!({"lost_reason": "Price"})).await;
    let mut before = Vec::new();
    for d in [open, won, lost] {
        before.push(admin.get("Opportunity", d).await);
    }
    let stages_before = admin.list("SalesStage", json!({})).await;
    sql(k, "update e_opportunity set pipeline = null where tenant_id = $1", tenant).await;
    sql(k, "update e_sales_stage set pipeline = null where tenant_id = $1", tenant).await;
    sql(k, "delete from e_sales_pipeline where tenant_id = $1", tenant).await;
    let mut tx = k.db.system_tx().await.unwrap();
    sqlx::raw_sql(include_str!("../packages/crm/sql/004_pipelines.sql")).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let sales = admin.one("SalesPipeline", json!({})).await;
    assert_eq!((sales["pipeline_name"].clone(), sales["is_default"].clone()), (json!("Sales Pipeline"), json!(true)), "one pipeline, the default");
    let without = |mut d: Doc| {
        d.remove("pipeline");
        d.remove("_titles");
        d
    };
    let stages_after = admin.list("SalesStage", json!({})).await;
    assert!(stages_after.iter().all(|s| s["pipeline"] == sales["id"]), "every stage in it");
    assert_eq!(
        stages_after.into_iter().map(without).collect::<Vec<_>>(),
        stages_before.into_iter().map(without).collect::<Vec<_>>(),
        "stages otherwise untouched"
    );
    for (d, was) in [open, won, lost].into_iter().zip(before) {
        let now = admin.get("Opportunity", d).await;
        assert_eq!(now["pipeline"], sales["id"], "every deal in it");
        assert_eq!(without(now), without(was), "nothing else changed");
    }
    // The won deal reopens where it last stood open, in its pipeline.
    anna.action("Opportunity", won, "reopen", json!({})).await;
    assert_eq!(anna.get("Opportunity", won).await["sales_stage"], negotiation);
    let (proposal, negotiation) = (stage(&sales["id"], "Proposal").await, stage(&sales["id"], "Negotiation").await);
    let prospecting = stage(&sales["id"], "Prospecting").await;
    let qualification = stage(&sales["id"], "Qualification").await;

    // ───────────── a second pipeline, with a stage of the same name ─────────────
    refused(anna.k.insert(&anna.ctx, "SalesPipeline", obj(json!({"pipeline_name": "Mine"}))).await, "not permitted");
    let renewals = maria.insert("SalesPipeline", json!({"pipeline_name": "Renewals"})).await;
    assert_eq!(renewals["is_default"], json!(false));
    let rp = renewals["id"].clone();
    for (name, order, probability, outcome, category) in [
        ("Up for Renewal", 1, "50", "Open", "Pipeline"),
        ("Proposal", 2, "70", "Open", "Commit"),
        ("Renewed", 3, "100", "Won", "Pipeline"),
        ("Churned", 4, "0", "Lost", "Pipeline"),
    ] {
        maria
            .insert(
                "SalesStage",
                json!({"pipeline": rp, "stage_name": name, "stage_order": order, "probability": probability, "outcome": outcome, "forecast_category": category}),
            )
            .await;
    }
    let twice = maria.k.insert(&maria.ctx, "SalesStage", obj(json!({"pipeline": rp, "stage_name": "Proposal", "probability": "1"}))).await;
    assert!(matches!(&twice, Err(Error::Conflict(m)) if m.contains("already has this Pipeline and Stage Name")), "{twice:?}");
    let (up, r_proposal) = (stage(&rp, "Up for Renewal").await, stage(&rp, "Proposal").await);
    assert_ne!(r_proposal, proposal, "two stages called Proposal, one in each");
    // A stage made without a pipeline is the default's.
    let demo = maria.insert("SalesStage", json!({"stage_name": "Demo", "probability": "30", "stage_order": 3})).await;
    assert_eq!(demo["pipeline"], sales["id"]);

    // A deal is where its stage is; with neither, at the default's first open stage.
    let r1 = uid(&anna.insert("Opportunity", deal("Acme renewal", json!({"sales_stage": r_proposal, "amount": "1000"}))).await);
    let r1d = anna.get("Opportunity", r1).await;
    assert_eq!((r1d["pipeline"].clone(), dec(&r1d["probability"])), (rp.clone(), n("70")));
    let plain = anna.insert("Opportunity", deal("Plain", json!({}))).await;
    assert_eq!((plain["pipeline"].clone(), plain["sales_stage"].clone()), (sales["id"].clone(), prospecting.clone()));
    refused(
        anna.k.insert(&anna.ctx, "Opportunity", obj(deal("Mixed", json!({"pipeline": rp, "sales_stage": proposal})))).await,
        "is not a stage of pipeline Renewals",
    );
    // Moved to another pipeline: its first open stage, and its history says so.
    let moved = anna.update("Opportunity", uid(&plain), json!({"pipeline": rp})).await;
    assert_eq!((moved["sales_stage"].clone(), dec(&moved["probability"])), (up.clone(), n("50")));
    let story = k.audit_trail(&anna.ctx, "Opportunity", uid(&plain)).await.unwrap();
    assert!(story["events"].as_array().unwrap().iter().any(|e| e["diff"].get("pipeline").is_some() && e["diff"].get("sales_stage").is_some()), "{story}");
    // With a stage of it given: that stage.
    let given = anna.update("Opportunity", open, json!({"pipeline": rp, "sales_stage": r_proposal})).await;
    assert_eq!((given["pipeline"].clone(), given["sales_stage"].clone()), (rp.clone(), r_proposal.clone()));
    // Another pipeline's stage chosen alone takes the deal there.
    let back = anna.update("Opportunity", open, json!({"sales_stage": negotiation})).await;
    assert_eq!(back["pipeline"], sales["id"]);
    // Won, reopened and lost within its pipeline.
    anna.action("Opportunity", r1, "mark_won", json!({})).await;
    assert_eq!(anna.get("Opportunity", r1).await["sales_stage"], stage(&rp, "Renewed").await);
    anna.action("Opportunity", r1, "reopen", json!({})).await;
    assert_eq!(anna.get("Opportunity", r1).await["sales_stage"], r_proposal, "where it stood, in Renewals");
    // The board's data, one pipeline at a time.
    let board = |p: Value| {
        let maria = &maria;
        async move {
            let stages: Vec<String> = maria
                .list("SalesStage", json!({"pipeline": p, "disabled": false, "outcome": "Open"}))
                .await
                .iter()
                .map(|s| s["stage_name"].as_str().unwrap().to_string())
                .collect();
            let mut deals: Vec<String> =
                maria.list("Opportunity", json!({"pipeline": p, "status": "Open"})).await.iter().map(|d| d["title"].as_str().unwrap().to_string()).collect();
            deals.sort();
            (stages.len(), deals)
        }
    };
    assert_eq!(board(rp.clone()).await, (2, vec!["Acme renewal".to_string(), "Plain".to_string()]));
    assert_eq!(board(sales["id"].clone()).await, (6, vec!["Open one".to_string(), "Won one".to_string()]));
    let span = [("company", cid.clone())];
    let pipe = maria.report("sales_pipeline", &[span[0].clone(), ("pipeline", rp.as_str().unwrap().to_string())]).await;
    let names: Vec<&str> = pipe["rows"].as_array().unwrap().iter().map(|r| r["sales_stage"].as_str().unwrap()).collect();
    assert_eq!(names, ["Up for Renewal", "Proposal"], "the pipeline's own stages");
    assert_eq!((pipe["totals"]["count"].clone(), dec(&pipe["totals"]["amount"])), (json!(2), n("1000")));
    let all = maria.report("sales_pipeline", &span).await;
    assert!(all["rows"].as_array().unwrap().iter().any(|r| r["sales_stage"] == json!("Renewals: Proposal")), "named with their pipeline: {all}");

    // Neither a stage nor a pipeline deals stand in is disabled or deleted; the refusal counts them.
    refused(maria.k.update(&maria.ctx, "SalesStage", idv(&r_proposal), obj(json!({"disabled": true})), None).await, "1 open deal stands in stage Proposal");
    refused(maria.k.delete(&maria.ctx, "SalesStage", idv(&up), None).await, "1 deal stands in stage Up for Renewal");
    refused(maria.k.update(&maria.ctx, "SalesPipeline", uid(&renewals), obj(json!({"disabled": true})), None).await, "2 open deals stand in pipeline Renewals");
    refused(maria.k.delete(&maria.ctx, "SalesPipeline", uid(&renewals), None).await, "2 deals stand in pipeline Renewals");
    refused(maria.k.update(&maria.ctx, "SalesPipeline", uid(&sales), obj(json!({"disabled": true})), None).await, "is the default");
    refused(maria.k.update(&maria.ctx, "SalesPipeline", uid(&sales), obj(json!({"is_default": false})), None).await, "make another one the default");
    // One default: making Renewals the default makes it the only one.
    maria.update("SalesPipeline", uid(&renewals), json!({"is_default": true})).await;
    assert_eq!(maria.get("SalesPipeline", uid(&sales)).await["is_default"], json!(false));
    maria.update("SalesPipeline", uid(&sales), json!({"is_default": true})).await;
    assert_eq!(maria.get("SalesPipeline", uid(&renewals)).await["is_default"], json!(false));
    // An empty pipeline goes with its stages; a disabled one takes no deal.
    let trials = maria.insert("SalesPipeline", json!({"pipeline_name": "Trials"})).await;
    let trial = maria.insert("SalesStage", json!({"pipeline": trials["id"], "stage_name": "Trying", "probability": "5"})).await;
    maria.update("SalesPipeline", uid(&trials), json!({"disabled": true})).await;
    refused(anna.k.update(&anna.ctx, "Opportunity", open, obj(json!({"pipeline": trials["id"]})), None).await, "pipeline Trials is disabled");
    maria.k.delete(&maria.ctx, "SalesPipeline", uid(&trials), None).await.expect("an empty pipeline goes");
    refused(maria.k.get(&maria.ctx, "SalesStage", uid(&trial), false).await, "not found");

    // ───────────── the forecast: by owner and category, on known figures ─────────────
    let co2 = admin.insert("Company", json!({"company_name": "Forecast Co", "abbr": "FC", "default_currency": inr["id"]})).await;
    let c2 = uid(&co2).to_string();
    let day = today().to_string();
    let forecast_deal = |who: &'static str, amount: &'static str, at: Value, extra: Value| {
        let (maria, c2, acme, day) = (&maria, c2.clone(), acme.clone(), day.clone());
        async move {
            let mut v = json!({"opportunity_from": "Customer", "customer": acme, "company": c2, "amount": amount, "assigned_to": who, "sales_stage": at,
                               "expected_close_date": day, "title": format!("{who} {amount}")});
            v.as_object_mut().unwrap().extend(extra.as_object().cloned().unwrap());
            uid(&maria.insert("Opportunity", v).await)
        }
    };
    let needs = stage(&sales["id"], "Needs Analysis").await;
    forecast_deal("anna@pipes.test", "1000", negotiation.clone(), json!({})).await; // Commit, 80%
    forecast_deal("anna@pipes.test", "2000", proposal.clone(), json!({})).await; // Best Case, 60%
    forecast_deal("anna@pipes.test", "500", prospecting.clone(), json!({"forecast_category": "Commit"})).await; // a person's call, 10%
    forecast_deal("anna@pipes.test", "700", qualification.clone(), json!({"forecast_category": "Omitted"})).await;
    forecast_deal("anna@pipes.test", "999", r_proposal.clone(), json!({})).await; // Renewals' Proposal: Commit, 70%
    let closed = forecast_deal("ben@pipes.test", "3000", negotiation.clone(), json!({})).await;
    maria.action("Opportunity", closed, "mark_won", json!({})).await;
    forecast_deal("ben@pipes.test", "400", prospecting.clone(), json!({"expected_close_date": null})).await;
    forecast_deal("ben@pipes.test", "600", needs.clone(), json!({})).await; // Pipeline, 40%
    let gone = forecast_deal("ben@pipes.test", "900", proposal.clone(), json!({})).await;
    maria.action("Opportunity", gone, "mark_lost", json!({"lost_reason": "Price"})).await;
    let period = |extra: &[(&'static str, String)]| {
        let mut p = vec![
            ("company", c2.clone()),
            ("from_date", (today() - chrono::Duration::days(30)).to_string()),
            ("to_date", (today() + chrono::Duration::days(30)).to_string()),
        ];
        p.extend_from_slice(extra);
        p
    };
    let figures = |r: &Value, group: &str| {
        let row = if group == "Total" {
            r["totals"].clone()
        } else {
            r["rows"].as_array().unwrap().iter().find(|x| x["group"] == json!(group)).cloned().unwrap_or_else(|| panic!("{group} in {r}"))
        };
        (row["count"].as_i64().unwrap(), ["closed", "commit", "best_case", "pipeline", "weighted"].map(|f| dec(&row[f])))
    };
    let f = maria.report("sales_forecast", &period(&[])).await;
    assert_eq!(figures(&f, "anna@pipes.test"), (4, [n("0"), n("2499"), n("2000"), n("0"), n("2749.30")]), "{f}");
    assert_eq!(figures(&f, "ben@pipes.test"), (1, [n("3000"), n("0"), n("0"), n("600"), n("240")]), "{f}");
    assert_eq!(figures(&f, "Total"), (5, [n("3000"), n("2499"), n("2000"), n("600"), n("2989.30")]));
    assert_eq!(f["meta"]["without_a_date"], json!(1), "counted apart");
    let only = maria.report("sales_forecast", &period(&[("pipeline", sales["id"].as_str().unwrap().to_string())])).await;
    assert_eq!(figures(&only, "anna@pipes.test"), (3, [n("0"), n("1500"), n("2000"), n("0"), n("2050")]), "{only}");
    let bens = maria.report("sales_forecast", &period(&[("assigned_to", "Ben@pipes.test".into())])).await;
    assert_eq!(bens["rows"].as_array().map(Vec::len), Some(1));
    let mine = anna.report("sales_forecast", &period(&[])).await;
    assert!(mine["rows"].as_array().unwrap().iter().all(|r| r["group"] != json!("ben@pipes.test")), "a rep's forecast is their own: {mine}");

    // ───────────── contacts with buying roles ─────────────
    let role = |name: &'static str| {
        let admin = &admin;
        async move { admin.one("BuyingRole", json!({"role_name": name})).await["id"].clone() }
    };
    let ann = admin.insert("Contact", json!({"first_name": "Ann", "customer": acme})).await["id"].clone();
    let raj = admin.insert("Contact", json!({"first_name": "Raj", "customer": acme})).await["id"].clone();
    let rollout = uid(&anna.insert("Opportunity", deal("Acme rollout", json!({}))).await);
    let two = anna
        .update(
            "Opportunity",
            rollout,
            json!({"contacts": [{"contact": ann, "role": role("Decision Maker").await}, {"contact": raj, "role": role("Finance").await}]}),
        )
        .await;
    assert_eq!(two["contact"], ann, "the first is the primary");
    assert_eq!(two["contacts"].as_array().unwrap().iter().filter(|c| c["is_primary"] == json!(true)).count(), 1);
    let rows = two["contacts"].as_array().unwrap().clone();
    let swapped =
        anna.update("Opportunity", rollout, json!({"contacts": [{"id": rows[0]["id"], "is_primary": false}, {"id": rows[1]["id"], "is_primary": true}]})).await;
    assert_eq!(swapped["contact"], raj, "the primary changed, and the deal's contact with it");
    refused(
        anna.k.update(&anna.ctx, "Opportunity", rollout, obj(json!({"contacts": [{"contact": ann}, {"contact": ann}]})), None).await,
        "on the deal already",
    );
    // A contact on many deals; the deal's contact set alone joins its contacts as the primary.
    let other = anna.update("Opportunity", open, json!({"contact": ann})).await;
    assert_eq!((other["contacts"][0]["contact"].clone(), other["contacts"][0]["is_primary"].clone()), (ann.clone(), json!(true)));

    // ───────────── what a stage asks of a deal entering it ─────────────
    maria.update("SalesStage", idv(&proposal), json!({"requires_amount": true, "requires_close_date": true, "requires_primary_contact": true})).await;
    maria.update("SalesStage", idv(&negotiation), json!({"requires_quotation": true, "requires_lines": true})).await;
    let bare = uid(&anna.insert("Opportunity", deal("Bare", json!({}))).await);
    refused(
        anna.k.update(&anna.ctx, "Opportunity", bare, obj(json!({"sales_stage": proposal})), None).await,
        "Proposal needs an amount, an expected close date and a primary contact before deal Bare enters it",
    );
    refused(anna.k.insert(&anna.ctx, "Opportunity", obj(deal("Straight in", json!({"sales_stage": proposal, "amount": "5"})))).await, "an expected close date");
    // Losing it, or moving it back, is never held up.
    let backwards = uid(&anna.insert("Opportunity", deal("Back", json!({"sales_stage": qualification}))).await);
    anna.update("Opportunity", backwards, json!({"sales_stage": prospecting})).await;
    anna.action("Opportunity", backwards, "mark_lost", json!({"lost_reason": "Timing"})).await;
    // Met: the stage takes it.
    let ready = anna.update("Opportunity", rollout, json!({"amount": "5000", "expected_close_date": day, "sales_stage": proposal})).await;
    assert_eq!(ready["sales_stage"], proposal);
    anna.update("Opportunity", rollout, json!({"sales_stage": qualification})).await;
    refused(
        anna.k.update(&anna.ctx, "Opportunity", rollout, obj(json!({"sales_stage": proposal, "expected_close_date": null})), None).await,
        "an expected close date",
    );
    anna.update("Opportunity", rollout, json!({"sales_stage": proposal})).await;

    // ───────────── lines priced from a price list in another currency ─────────────
    let yesterday = (today() - chrono::Duration::days(1)).to_string();
    admin.insert("ExchangeRate", json!({"from_currency": usd["id"], "to_currency": inr["id"], "rate": "83", "valid_from": yesterday})).await;
    let usd_list = admin.insert("PriceList", json!({"price_list_name": "Export USD", "currency": usd["id"], "selling": true})).await["id"].clone();
    admin.insert("ItemPrice", json!({"item": widget, "price_list": usd_list, "rate": "10", "valid_from": yesterday})).await;
    let priced = anna
        .update(
            "Opportunity",
            rollout,
            json!({"price_list": usd_list, "items": [{"item": widget, "qty": "2", "discount_percent": "10"}, {"item": widget, "uom": pack["id"], "qty": "1"},
                   {"item": gadget, "qty": "1", "rate": "35"}]}),
        )
        .await;
    let lines = priced["items"].as_array().unwrap().clone();
    assert_eq!(
        lines.iter().map(|l| (dec(&l["rate"]), dec(&l["amount"]))).collect::<Vec<_>>(),
        [(n("830"), n("1494")), (n("8300"), n("8300")), (n("35"), n("35"))],
        "10 USD at 83, a pack of ten, and a rate a person typed"
    );
    assert_eq!((lines[0]["uom"].clone(), lines[1]["uom"].clone()), (nos["id"].clone(), pack["id"].clone()));
    assert_eq!(dec(&priced["amount"]), n("9829"));
    // The item changed on a line whose rate was not touched: priced again; a typed rate stays.
    let repriced =
        anna.update("Opportunity", rollout, json!({"items": [{"id": lines[0]["id"]}, {"id": lines[1]["id"]}, {"id": lines[2]["id"], "item": widget}]})).await;
    assert_eq!(dec(&repriced["items"][2]["rate"]), n("830"), "another item, the rate left as it was: priced again");
    let repriced = anna
        .update(
            "Opportunity",
            rollout,
            json!({"items": [{"id": lines[0]["id"]}, {"id": lines[1]["id"]}, {"id": lines[2]["id"], "item": gadget, "rate": "36"}]}),
        )
        .await;
    assert_eq!(dec(&repriced["items"][2]["rate"]), n("36"), "a rate typed with it stays the person's");
    refused(
        anna.k.update(&anna.ctx, "Opportunity", rollout, obj(json!({"items": [{"item": gadget, "uom": pack["id"], "qty": "1"}]})), None).await,
        "no conversion",
    );
    // The same list for a deal in dollars: no conversion.
    let dollars =
        anna.insert("Opportunity", deal("In dollars", json!({"currency": usd["id"], "price_list": usd_list, "items": [{"item": widget, "qty": "3"}]}))).await;
    assert_eq!((dec(&dollars["items"][0]["rate"]), dec(&dollars["amount"])), (n("10"), n("30")));

    // ───────────── a quotation sent, rejected, revised twice and ordered ─────────────
    refused(anna.k.update(&anna.ctx, "Opportunity", rollout, obj(json!({"sales_stage": negotiation})), None).await, "a submitted quotation");
    admin.insert("QuotationApprovalRule", json!({"rule_name": "Discounts", "discount_over": "5", "approver_role": "Sales Manager"})).await;
    let q1 = vid(&anna.action("Opportunity", rollout, "make_quotation", json!({})).await);
    let quote = anna.get("Quotation", q1).await;
    assert_eq!((quote["price_list"].clone(), quote["contact"].clone(), quote["revision"].clone()), (usd_list.clone(), raj.clone(), json!(1)));
    let carried: Vec<_> =
        quote["items"].as_array().unwrap().iter().map(|l| (l["uom"].clone(), dec(&l["price_list_rate"]), dec(&l["discount_percent"]))).collect();
    let dealt: Vec<_> = repriced["items"].as_array().unwrap().iter().map(|l| (l["uom"].clone(), dec(&l["rate"]), dec(&l["discount_percent"]))).collect();
    assert_eq!(carried, dealt, "price list, units, rates and discounts carried unchanged");
    assert_eq!(dec(&quote["net_total"]), dec(&repriced["amount"]));
    anna.update("Quotation", q1, json!({"discount_percent": "8"})).await;
    refused(anna.k.submit(&anna.ctx, "Quotation", q1, None).await, "approval");
    maria.action("Quotation", q1, "approve_quotation", json!({})).await;
    anna.submit("Quotation", q1).await;
    let entered = anna.update("Opportunity", rollout, json!({"sales_stage": negotiation})).await;
    assert_eq!(entered["sales_stage"], negotiation, "a submitted quotation and lines: Negotiation takes it");
    assert_eq!(anna.action("Quotation", q1, "mark_sent", json!({})).await["status"], json!("Sent"));
    assert_eq!(anna.get("Quotation", q1).await["sent_on"], json!(day));
    refused(anna.k.run_action(&anna.ctx, "Quotation", q1, "mark_rejected", json!({})).await, "lost_reason");
    let said = anna.action("Quotation", q1, "mark_rejected", json!({"lost_reason": "Price", "lost_detail": "Wants 15% off"})).await;
    assert_eq!(said["status"], json!("Rejected"));
    let rejected = anna.get("Quotation", q1).await;
    assert_eq!(
        (rejected["answer"].clone(), rejected["answered_by"].clone(), rejected["answered_on"].clone()),
        (json!("Rejected"), json!("anna@pipes.test"), json!(day))
    );
    assert_eq!(anna.get("Opportunity", rollout).await["status"], json!("Open"), "rejecting its only quotation does not lose the deal");
    refused(anna.k.run_action(&anna.ctx, "Quotation", q1, "make_sales_order", json!({})).await, "the customer rejected quotation");
    refused(anna.k.run_action(&anna.ctx, "Quotation", q1, "mark_accepted", json!({})).await, "not available");
    // Revised: the next number, linked both ways, and approved again.
    anna.cancel("Quotation", q1).await;
    let q2 = uid(&k.amend(&anna.ctx, "Quotation", q1).await.expect("amend"));
    let second = anna.get("Quotation", q2).await;
    assert_eq!((second["revision"].clone(), second["revision_of"].clone(), second["status"].clone()), (json!(2), json!(q1.to_string()), json!("Draft")));
    assert!(second["approved_by"].is_null() && second["answer"].is_null() && second["sent_on"].is_null(), "{second:?}");
    assert_eq!(anna.get("Quotation", q1).await["replaced_by"], json!(q2.to_string()), "the old one shows it was replaced");
    anna.update("Quotation", q2, json!({"discount_percent": "12"})).await;
    refused(anna.k.submit(&anna.ctx, "Quotation", q2, None).await, "approval");
    maria.action("Quotation", q2, "approve_quotation", json!({})).await;
    anna.submit("Quotation", q2).await;
    anna.action("Quotation", q2, "mark_sent", json!({})).await;
    anna.cancel("Quotation", q2).await;
    let q3 = uid(&k.amend(&anna.ctx, "Quotation", q2).await.expect("amend"));
    assert_eq!(anna.get("Quotation", q3).await["revision"], json!(3));
    refused(anna.k.submit(&anna.ctx, "Quotation", q3, None).await, "approval");
    maria.action("Quotation", q3, "approve_quotation", json!({})).await;
    anna.submit("Quotation", q3).await;
    let latest: Vec<Value> = anna.list("Quotation", json!({"opportunity": rollout.to_string()})).await.iter().map(|q| q["revision"].clone()).collect();
    assert_eq!(latest, [json!(3), json!(2), json!(1)], "the latest revision first");
    // Quotations from before 0.10.0 get their revisions and links from the platform's amendments.
    sql(k, "update e_quotation set revision = null, revision_of = null, replaced_by = null where opportunity = $1", rollout).await;
    let mut tx = k.db.system_tx().await.unwrap();
    sqlx::raw_sql(include_str!("../packages/crm/sql/005_quotation_revisions.sql")).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let (one, two, three) = (anna.get("Quotation", q1).await, anna.get("Quotation", q2).await, anna.get("Quotation", q3).await);
    assert_eq!([&one["revision"], &two["revision"], &three["revision"]], [&json!(1), &json!(2), &json!(3)]);
    assert_eq!(
        (three["revision_of"].clone(), one["replaced_by"].clone(), two["replaced_by"].clone()),
        (json!(q2.to_string()), json!(q2.to_string()), json!(q3.to_string()))
    );
    assert_eq!(anna.action("Quotation", q3, "mark_accepted", json!({})).await["status"], json!("Accepted"));
    let order = vid(&anna.action("Quotation", q3, "make_sales_order", json!({})).await);
    anna.submit("SalesOrder", order).await;
    assert_eq!(anna.get("Quotation", q3).await["status"], json!("Ordered"));
    let won_now = anna.get("Opportunity", rollout).await;
    assert_eq!(
        (won_now["status"].clone(), won_now["sales_stage"].clone()),
        (json!("Won"), stage(&sales["id"], "Won").await),
        "won by its order, in its pipeline"
    );
    // A sent quotation expires past its validity; an expired one is not ordered, and says why.
    let late = uid(&anna.insert("Quotation", json!({"customer": acme, "items": [{"item": widget, "qty": "1"}], "valid_till": day})).await);
    anna.submit("Quotation", late).await;
    anna.action("Quotation", late, "mark_sent", json!({})).await;
    sql(k, "update e_quotation set valid_till = current_date - 3 where id = $1", late).await;
    ferrum_app_crm::maintenance(k).await.expect("maintenance");
    assert_eq!(anna.get("Quotation", late).await["status"], json!("Expired"));
    refused(anna.k.run_action(&anna.ctx, "Quotation", late, "make_sales_order", json!({})).await, "expired");
    refused(anna.k.run_action(&anna.ctx, "Quotation", late, "mark_accepted", json!({})).await, "not available");
    let reg = maria.report("quotation_register", &[("company", cid.clone()), ("status", "Rejected".into())]).await;
    assert_eq!(reg["totals"]["count"], json!(0), "a rejected revision since replaced is cancelled");
}

/// A sales team arriving from another CRM, with its accounts and its ways: accounts with an owner, a
/// territory and a parent; people at several companies, and moving between them; managers and
/// territories seeing their people's records and nobody else's; targets; consent to calls and e-mail;
/// prospects merged; leads scored. An accountant reads customers as before.
async fn accounts_and_teams(k: &Arc<Kernel>) {
    let tenant = k
        .create_tenant(NewTenant {
            slug: "accts".into(),
            name: "Accounts".into(),
            admin_email: "admin@accts.test".into(),
            admin_password: PASSWORD.into(),
            admin_name: None,
            packages: vec!["crm".into(), "crm_erp".into()],
            settings: Some(json!({"fiscal_year_start_month": 4, "timezone": *ZONE})),
        })
        .await
        .expect("tenant");
    let session = k.login("accts", "admin@accts.test", PASSWORD).await.expect("login");
    let admin = U { k: k.clone(), ctx: k.load_ctx(tenant, session.user, Some(session.token_version), "it".into()).await.expect("ctx") };
    let inr = admin.one("Currency", json!({"code": "INR"})).await;
    let usd = admin.one("Currency", json!({"code": "USD"})).await;
    let company = admin.insert("Company", json!({"company_name": "Accounts", "abbr": "AC", "default_currency": inr["id"]})).await;
    let cid = uid(&company).to_string();
    let group = admin.one("ItemGroup", json!({"group_name": "Products"})).await;
    let nos = admin.one("Uom", json!({"uom_name": "Nos"})).await;
    let widget = uid(&admin
        .insert("Item", json!({"item_code": "W", "item_name": "Widget", "item_group": group["id"], "stock_uom": nos["id"], "standard_rate": "100"}))
        .await)
    .to_string();
    let user = |email: &'static str, roles: &'static [&'static str]| {
        let (k, admin_ctx) = (k.clone(), admin.ctx.clone());
        async move {
            let roles: Vec<String> = roles.iter().map(|r| r.to_string()).collect();
            let id = k.create_user(&admin_ctx, email, email, Some(PASSWORD), &roles, false).await.expect("user");
            U { ctx: k.load_ctx(tenant, id, None, "it".into()).await.expect("user ctx"), k }
        }
    };
    let anna = user("anna@accts.test", &["Sales User"]).await;
    let ben = user("ben@accts.test", &["Sales User"]).await;
    let mo = user("mo@accts.test", &["Sales User"]).await;
    let tara = user("tara@accts.test", &["Sales User"]).await;
    let dan = user("dan@accts.test", &["Sales User"]).await;
    let maria = user("maria@accts.test", &["Sales Manager"]).await;
    let acc = user("acc@accts.test", &["Accounts User"]).await;
    let titles = |rows: &[Doc]| -> Vec<String> {
        let mut t: Vec<String> = rows.iter().filter_map(|r| r.get("title").or(r.get("lead_name")).and_then(Value::as_str).map(str::to_string)).collect();
        t.sort();
        t
    };

    // ───────────── accounts: an owner, a territory and a parent, never a cycle ─────────────
    let all = admin.one("Territory", json!({"territory_name": "All Territories"})).await;
    let india = maria
        .insert("Territory", json!({"territory_name": "India", "is_group": true, "parent_territory": all["id"], "people": [{"person": "Tara@accts.test"}]}))
        .await;
    let karnataka = maria.insert("Territory", json!({"territory_name": "Karnataka", "parent_territory": india["id"]})).await;
    let europe = maria.insert("Territory", json!({"territory_name": "Europe", "parent_territory": all["id"]})).await;
    let reach = |t: &Doc| t["reach"].as_array().unwrap().iter().map(|r| r["person"].as_str().unwrap().to_string()).collect::<Vec<_>>();
    assert_eq!(reach(&maria.get("Territory", uid(&karnataka)).await), ["tara@accts.test"], "a territory reaches the people of those above it");
    refused(k.update(&maria.ctx, "Territory", uid(&india), obj(json!({"parent_territory": karnataka["id"]})), None).await, "cycle in the hierarchy");

    let acme = maria.insert("Customer", json!({"customer_name": "Acme Group", "account_owner": "anna@accts.test", "territory": india["id"]})).await;
    let retail = maria.insert("Customer", json!({"customer_name": "Acme Retail", "parent_customer": acme["id"]})).await;
    let shop = maria.insert("Customer", json!({"customer_name": "Acme Shop", "parent_customer": retail["id"]})).await;
    refused(
        k.update(&maria.ctx, "Customer", uid(&acme), obj(json!({"parent_customer": shop["id"]})), None).await,
        "this would create a cycle in the hierarchy",
    );
    refused(k.update(&maria.ctx, "Customer", uid(&acme), obj(json!({"parent_customer": acme["id"]})), None).await, "cannot be its own parent");
    assert_eq!(maria.one("Customer", json!({"parent_customer": acme["id"]})).await["customer_name"], json!("Acme Retail"), "a parent lists its children");
    let holding = maria.insert("Prospect", json!({"prospect_name": "Globex Holding"})).await;
    let globex = maria.insert("Prospect", json!({"prospect_name": "Globex Labs", "parent_prospect": holding["id"]})).await;
    refused(k.update(&maria.ctx, "Prospect", uid(&holding), obj(json!({"parent_prospect": globex["id"]})), None).await, "cycle in the hierarchy");
    // An accountant reads every customer, with what CRM added, as before.
    assert_eq!(acc.list("Customer", json!({})).await.len(), admin.list("Customer", json!({})).await.len());
    assert_eq!(acc.get("Customer", uid(&retail)).await["parent_customer"], acme["id"]);

    // ───────────── converting a lead: its owner and territory for a new customer ─────────────
    let lia = anna
        .insert(
            "Lead",
            json!({"first_name": "Lia", "last_name": "Rao", "organization_name": "Lia Studio", "email": "lia@studio.test", "company": cid, "territory": karnataka["id"],
                   "do_not_call": true, "call_consent_how": "Phone Call", "unsubscribed": true}),
        )
        .await;
    assert_eq!((lia["call_consent_on"].clone(), lia["email_consent_on"].clone()), (json!(today().to_string()), json!(today().to_string())), "each side dated");
    let made = anna.action("Lead", uid(&lia), "convert_to_customer", json!({})).await;
    let studio = maria.get("Customer", vid(&made)).await;
    assert_eq!((studio["account_owner"].clone(), studio["territory"].clone()), (json!("anna@accts.test"), karnataka["id"].clone()));
    let lia_ct = maria.get("Contact", vid(&json!({"id": made["contact"]}))).await;
    assert_eq!(
        (lia_ct["do_not_call"].clone(), lia_ct["call_consent_how"].clone(), lia_ct["call_consent_on"].clone(), lia_ct["unsubscribed"].clone()),
        (json!(true), json!("Phone Call"), json!(today().to_string()), json!(true)),
        "consent goes on with the person"
    );
    let at = maria.one("AccountContact", json!({"contact": lia_ct["id"]})).await;
    assert_eq!((at["customer"].clone(), at["is_primary"].clone()), (studio["id"].clone(), json!(true)));
    let gus = ben.insert("Lead", json!({"first_name": "Gus", "organization_name": "Acme Group", "company": cid, "territory": europe["id"]})).await;
    ben.action("Lead", uid(&gus), "convert_to_customer", json!({"customer": acme["id"]})).await;
    let acme_now = maria.get("Customer", uid(&acme)).await;
    assert_eq!(
        (acme_now["account_owner"].clone(), acme_now["territory"].clone()),
        (json!("anna@accts.test"), india["id"].clone()),
        "an existing customer's own stay"
    );

    // ───────────── a contact at two companies, moved to a third ─────────────
    let sam = maria.insert("Contact", json!({"first_name": "Sam", "last_name": "Lee", "designation": "CFO", "customer": acme["id"]})).await;
    let links = |c: Value| {
        let maria = &maria;
        async move {
            let mut l = maria.list("AccountContact", json!({"contact": c})).await;
            l.sort_by_key(|r| r["customer"].as_str().map(str::to_string));
            l
        }
    };
    let first = maria.one("AccountContact", json!({"contact": sam["id"]})).await;
    assert_eq!((first["customer"].clone(), first["title"].clone(), first["is_primary"].clone()), (acme["id"].clone(), json!("CFO"), json!(true)));
    maria.insert("AccountContact", json!({"contact": sam["id"], "customer": retail["id"], "title": "Board Member", "start_date": "2025-01-01"})).await;
    assert_eq!(maria.get("Contact", uid(&sam)).await["customer"], acme["id"], "the primary stays their Customer");
    refused(k.insert(&maria.ctx, "AccountContact", obj(json!({"contact": sam["id"], "customer": retail["id"]}))).await, "Sam Lee is at Acme Retail already");
    refused(k.insert(&maria.ctx, "AccountContact", obj(json!({"contact": sam["id"]}))).await, "Choose a customer or a prospect");
    let initech = maria.insert("Customer", json!({"customer_name": "Initech"})).await;
    let moved = maria.action("Contact", uid(&sam), "move_to_account", json!({"customer": initech["id"], "title": "CEO"})).await;
    assert_eq!(moved["message"], json!(format!("Sam Lee moved to Initech on {}", today())));
    let sam_now = maria.get("Contact", uid(&sam)).await;
    assert_eq!((sam_now["customer"].clone(), sam_now["designation"].clone()), (initech["id"].clone(), json!("CEO")));
    let history = links(sam["id"].clone()).await;
    assert_eq!(history.len(), 3, "the history is kept: {history:?}");
    let link = |c: &Doc| history.iter().find(|l| l["customer"] == c["id"]).unwrap().clone();
    assert_eq!((link(&acme)["end_date"].clone(), link(&acme)["is_primary"].clone()), (json!(today().to_string()), json!(false)), "the old company ends");
    assert!(link(&retail)["end_date"].is_null(), "a seat elsewhere stays");
    assert_eq!((link(&initech)["start_date"].clone(), link(&initech)["is_primary"].clone()), (json!(today().to_string()), json!(true)));
    assert_eq!(maria.list("AccountContact", json!({"customer": acme["id"]})).await.len(), 2, "the account lists who was there too");
    // Their Customer changed by hand to a company they are at besides is their primary changing.
    maria.update("Contact", uid(&sam), json!({"customer": retail["id"]})).await;
    let history = links(sam["id"].clone()).await;
    let primary: Vec<&Value> = history.iter().filter(|l| l["is_primary"] == json!(true)).map(|l| &l["customer"]).collect();
    assert_eq!(primary, [&retail["id"]]);
    assert!(history.iter().find(|l| l["customer"] == initech["id"]).unwrap()["end_date"].is_null(), "still at Initech");

    // Contacts already there get a link to their customer, the primary (sql/006_account_contacts.sql), once.
    let olga = maria.insert("Contact", json!({"first_name": "Olga", "designation": "Buyer", "customer": retail["id"]})).await;
    sql(k, "delete from e_account_contact where contact = $1", uid(&olga)).await;
    for _ in 0..2 {
        let mut tx = k.db.system_tx().await.unwrap();
        sqlx::raw_sql(include_str!("../packages/crm/sql/006_account_contacts.sql")).execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
    }
    let back = maria.one("AccountContact", json!({"contact": olga["id"]})).await;
    assert_eq!(
        (back["customer"].clone(), back["is_primary"].clone(), back["title"].clone(), back["start_date"].clone()),
        (retail["id"].clone(), json!(true), json!("Buyer"), Value::Null)
    );
    // A contact deleted takes its links along.
    k.delete(&maria.ctx, "Contact", uid(&olga), None).await.expect("deleted with its link");
    assert!(maria.list("AccountContact", json!({"contact": olga["id"]})).await.is_empty());

    // ───────────── sales teams and territories: whose records each sees ─────────────
    let north = maria.insert("SalesTeam", json!({"team_name": "North", "manager": "Mo@accts.test", "members": [{"person": "anna@accts.test"}]})).await;
    let south = maria.insert("SalesTeam", json!({"team_name": "South", "manager": "maria@accts.test", "members": [{"person": "ben@accts.test"}]})).await;
    assert_eq!(maria.get("Lead", uid(&lia)).await["sales_team"], north["id"], "records already there join their owner's team");
    let deal = |title: &str, extra: Value| {
        let mut v = json!({"opportunity_from": "Customer", "customer": acme["id"], "title": title, "company": cid});
        v.as_object_mut().unwrap().extend(extra.as_object().cloned().unwrap());
        v
    };
    let north_deal = anna.insert("Opportunity", deal("North deal", json!({"amount": "100"}))).await;
    assert_eq!(north_deal["sales_team"], north["id"]);
    let south_deal = ben.insert("Opportunity", deal("South deal", json!({"amount": "100"}))).await;
    assert_eq!(south_deal["sales_team"], south["id"]);
    let call = anna.insert("Activity", json!({"activity_type": "Task", "subject": "Send the deck", "opportunity": north_deal["id"]})).await;
    let quote =
        anna.insert("Quotation", json!({"company": cid, "party_type": "Customer", "customer": acme["id"], "items": [{"item": widget, "qty": "1"}]})).await;
    assert_eq!((call["sales_team"].clone(), quote["sales_team"].clone()), (north["id"].clone(), north["id"].clone()));
    // The manager: the team's deals, leads, activities and quotations, and nothing of South's.
    assert_eq!(titles(&mo.list("Opportunity", json!({})).await), ["North deal"]);
    assert_eq!(titles(&mo.list("Lead", json!({})).await), ["Lia Rao"]);
    assert_eq!(mo.list("Activity", json!({})).await.len(), 1);
    assert_eq!(mo.list("Quotation", json!({})).await.len(), 1);
    mo.update("Opportunity", uid(&north_deal), json!({"next_step": "Call back Friday"})).await;
    assert!(k.get(&mo.ctx, "Opportunity", uid(&south_deal), false).await.is_err(), "another team's deal");
    assert!(k.update(&mo.ctx, "Opportunity", uid(&south_deal), obj(json!({"next_step": "x"})), None).await.is_err());
    // A rep on another team sees their own only.
    assert_eq!(titles(&ben.list("Opportunity", json!({})).await), ["South deal"]);
    assert!(k.get(&ben.ctx, "Opportunity", uid(&north_deal), false).await.is_err());
    // A territory's rep: the leads and deals in it and under it, besides their own.
    let ka_lead = ben.insert("Lead", json!({"first_name": "Kiran", "company": cid, "territory": karnataka["id"]})).await;
    ben.insert("Lead", json!({"first_name": "Elsa", "company": cid, "territory": europe["id"]})).await;
    ben.insert("Opportunity", deal("Bengaluru deal", json!({"territory": karnataka["id"]}))).await;
    assert_eq!(titles(&tara.list("Lead", json!({})).await), ["Kiran", "Lia Rao"]);
    assert_eq!(titles(&tara.list("Opportunity", json!({})).await), ["Bengaluru deal"]);
    tara.update("Lead", uid(&ka_lead), json!({"next_follow_up": today().to_string()})).await;
    // Taken off the territory, she sees none of it.
    maria.update("Territory", uid(&india), json!({"people": []})).await;
    assert!(reach(&maria.get("Territory", uid(&karnataka)).await).is_empty(), "the territories under it follow");
    assert!(tara.list("Lead", json!({})).await.is_empty());

    // ───────────── handover: a rep who leaves hands over the customers they look after ─────────────
    let dans = maria.insert("Customer", json!({"customer_name": "Dan's Account", "account_owner": "dan@accts.test"})).await;
    let dans_lead = dan.insert("Lead", json!({"first_name": "Dina", "company": cid})).await;
    k.deprovision_user(&admin.ctx, dan.ctx.user, Some(ben.ctx.user), "left").await.expect("deprovisioned");
    while k.dispatch_outbox_once().await.unwrap() > 0 {}
    assert_eq!(maria.get("Customer", uid(&dans)).await["account_owner"], json!("ben@accts.test"));
    assert_eq!(maria.get("Lead", uid(&dans_lead)).await["assigned_to"], json!("ben@accts.test"));

    // ───────────── targets: met and missed, in the company's currency ─────────────
    let negotiation = admin.one("SalesStage", json!({"stage_name": "Negotiation"})).await["id"].clone();
    for (who, title, extra) in [
        (&anna, "Rupees", json!({"amount": "5000"})),
        (&anna, "Dollars", json!({"amount": "100", "currency": usd["id"], "conversion_rate": "80"})),
        (&ben, "Ben's one", json!({"amount": "10"})),
    ] {
        let d = who.insert("Opportunity", deal(title, extra)).await;
        who.action("Opportunity", uid(&d), "mark_won", json!({})).await;
    }
    anna.insert("Opportunity", deal("Committed", json!({"amount": "3000", "sales_stage": negotiation, "expected_close_date": today().to_string()}))).await;
    let target = |v: Value| {
        let mut t = json!({"company": cid, "period": "Quarter", "start_date": today().to_string()});
        t.as_object_mut().unwrap().extend(v.as_object().cloned().unwrap());
        obj(t)
    };
    let annas = maria.insert("SalesTarget", json!(target(json!({"person": "Anna@accts.test", "target": "20000"})))).await;
    assert_eq!(annas["start_date"].as_str().map(|d| d.ends_with("-01")), Some(true), "the quarter's first day");
    maria.insert("SalesTarget", json!(target(json!({"person": "ben@accts.test", "measure": "Deals Won", "target": "1"})))).await;
    maria.insert("SalesTarget", json!(target(json!({"target_for": "Team", "sales_team": north["id"], "target": "10000"})))).await;
    refused(k.insert(&maria.ctx, "SalesTarget", target(json!({"person": "anna@accts.test", "period": "Month", "target": "1"}))).await, "it overlaps target");
    maria.insert("SalesTarget", json!(target(json!({"person": "anna@accts.test", "measure": "Deals Won", "target": "5"})))).await;
    let report = maria.report("sales_targets", &[("company", cid.clone())]).await;
    let rows = report["rows"].as_array().unwrap().clone();
    let row = |who: &str, measure: &str| {
        rows.iter().find(|r| r["who"] == json!(who) && r["measure"] == json!(measure)).unwrap_or_else(|| panic!("{who} {measure}: {report}")).clone()
    };
    let a = row("anna@accts.test", "Amount Won");
    assert_eq!(
        (dec(&a["target"]), dec(&a["achieved"]), dec(&a["gap"]), dec(&a["commit"]), a["met"].clone()),
        (n("20000"), n("13000"), n("7000"), n("3000"), json!(false)),
        "missed: {a}"
    );
    let b = row("ben@accts.test", "Deals Won");
    assert_eq!((dec(&b["achieved"]), b["met"].clone()), (n("1"), json!(true)), "met: {b}");
    let team = row("North", "Amount Won");
    assert_eq!((dec(&team["achieved"]), team["met"].clone()), (n("13000"), json!(true)), "a team's is its members': {team}");
    assert_eq!(dec(&row("anna@accts.test", "Deals Won")["achieved"]), n("2"));
    // A rep sees their own targets and their team's.
    let mine = anna.report("sales_targets", &[("company", cid.clone())]).await;
    assert_eq!(mine["rows"].as_array().unwrap().len(), 3, "{mine}");

    // ───────────── do-not-call ─────────────
    let lia_deal = anna
        .insert(
            "Opportunity",
            json!({"opportunity_from": "Customer", "customer": studio["id"], "contact": lia_ct["id"], "company": cid, "title": "Studio renewal"}),
        )
        .await;
    let logged = |reason: Option<&str>| {
        obj(
            json!({"activity_type": "Call", "subject": "Renewal", "opportunity": lia_deal["id"], "call_reason": reason, "due_at": chrono::Utc::now().to_rfc3339()}),
        )
    };
    refused(k.insert(&anna.ctx, "Activity", logged(None)).await, "Lia Rao is on do-not-call");
    let call = anna.insert("Activity", json!(logged(Some("She asked us to call about the renewal")))).await;
    let pat = anna.insert("Lead", json!({"first_name": "Pat", "company": cid, "do_not_call": true})).await;
    refused(k.insert(&anna.ctx, "Activity", obj(json!({"activity_type": "Call", "subject": "Hello", "lead": pat["id"]}))).await, "call_reason");
    anna.insert("Activity", json!({"activity_type": "Email", "subject": "Hello", "lead": pat["id"]})).await;
    let site = k.hooks.site("crm").expect("CRM's endpoints").clone();
    let req = ferrum_kernel::SiteRequest {
        tenant,
        user: Some(anna.ctx.clone()),
        method: "GET".into(),
        path: "/work".into(),
        query: Default::default(),
        headers: Default::default(),
        body: vec![],
        client_ip: "203.0.113.7".into(),
    };
    let work: Value = serde_json::from_slice(&site.handle(k, req).await.expect("work").body).unwrap();
    let item = work["items"].as_array().unwrap().iter().find(|i| i["url"] == json!(format!("/crm/d/{}", lia_deal["id"].as_str().unwrap()))).cloned();
    assert!(item.as_ref().is_some_and(|i| i["subtitle"].as_str().unwrap().ends_with("Do not call")), "{work}");
    anna.action("Activity", uid(&call), "complete", json!({"call_outcome": "Reached"})).await;

    // ───────────── a prospect merged: its deals, quotations, leads and people follow ─────────────
    let dup = maria.insert("Prospect", json!({"prospect_name": "Globex Labs Inc"})).await;
    let pdeal = anna.insert("Opportunity", json!({"opportunity_from": "Prospect", "prospect": dup["id"], "company": cid})).await;
    let plead = anna.insert("Lead", json!({"first_name": "Gil", "company": cid, "prospect": dup["id"]})).await;
    let pquote =
        anna.insert("Quotation", json!({"company": cid, "party_type": "Prospect", "prospect": dup["id"], "items": [{"item": widget, "qty": "1"}]})).await;
    let gil = maria.insert("Contact", json!({"first_name": "Gil", "prospect": dup["id"]})).await;
    assert_eq!(maria.one("AccountContact", json!({"contact": gil["id"]})).await["prospect"], dup["id"]);
    k.merge_records(&maria.ctx, "Prospect", uid(&globex), uid(&dup), "The same company, entered twice").await.expect("merged");
    assert_eq!(maria.get("Opportunity", uid(&pdeal)).await["prospect"], globex["id"]);
    assert_eq!(maria.get("Lead", uid(&plead)).await["prospect"], globex["id"]);
    assert_eq!(maria.get("Quotation", uid(&pquote)).await["prospect"], globex["id"]);
    assert_eq!(maria.one("AccountContact", json!({"contact": gil["id"]})).await["prospect"], globex["id"]);
    assert!(k.merge_records(&maria.ctx, "Contact", uid(&gil), uid(&sam), "x").await.is_err(), "contacts are the platform's, and not mergeable");

    // ───────────── lead scoring ─────────────
    maria
        .insert("LeadScoringRule", json!({"description": "Came from the website", "field": "source", "comparison": "Is", "value": "Website", "points": 10}))
        .await;
    maria
        .insert("LeadScoringRule", json!({"description": "Big company", "field": "annual_revenue", "comparison": "At Least", "value": "1000000", "points": 15}))
        .await;
    refused(
        k.insert(&maria.ctx, "LeadScoringRule", obj(json!({"description": "x", "field": "nonsense", "points": 1}))).await,
        "the lead has no field nonsense",
    );
    refused(
        k.insert(&maria.ctx, "LeadScoringRule", obj(json!({"description": "x", "field": "budget", "comparison": "At Least", "value": "lots", "points": 1})))
            .await,
        "give a number",
    );
    let hot = anna.insert("Lead", json!({"first_name": "Hot", "company": cid, "source": "Website"})).await;
    assert_eq!((hot["score"].clone(), hot["score_why"].clone()), (json!(10), json!("Came from the website +10")));
    let intro = anna.insert("Activity", json!({"activity_type": "Call", "subject": "Intro", "lead": hot["id"], "status": "Done"})).await;
    let hot_now = anna.get("Lead", uid(&hot)).await;
    assert_eq!((hot_now["score"].clone(), hot_now["score_why"].clone()), (json!(20), json!("Came from the website +10; 1 recent activity +10")));
    sql(k, "update e_activity set completed_at = now() - interval '14 days', due_at = null where id = $1", uid(&intro)).await;
    anna.update("Activity", uid(&intro), json!({"details": "Two weeks on"})).await;
    assert_eq!(anna.get("Lead", uid(&hot)).await["score"], json!(15), "an activity's points halve as it ages");
    assert_eq!(titles(&anna.list("Lead", json!({"score": [">=", 15]})).await), ["Hot"]);

    // ───────────── an erasure takes when and how consent was given, never the refusal ─────────────
    k.erase_personal_data(&admin.ctx, "lia@studio.test", "Request 11").await.expect("erased");
    let lead = admin.get("Lead", uid(&lia)).await;
    assert!(lead["call_consent_on"].is_null() && lead["call_consent_how"].is_null() && lead["do_not_call"] == json!(true), "{lead:?}");
    let ct = admin.get("Contact", uid(&lia_ct)).await;
    assert!(ct["call_consent_on"].is_null() && ct["email_consent_on"].is_null() && ct["do_not_call"] == json!(true), "{ct:?}");
}
