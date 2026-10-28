        # Ferrum CRM

        A native app for [Ferrum](https://github.com/CBFerrum/Ferrum), the open-source, AI-first ERP.

        - Leads with duplicate-safe capture, qualification and one-step conversion to customer, contact and address
- Opportunities with configurable sales stages, probabilities, weighted pipeline, competitors and won/lost reasons
- Prospects, contacts (several emails and phones), addresses, campaigns and activities (calls, meetings, tasks)
- Quotations priced and taxed by the selling engine, turned into sales orders with ordered quantities tracked back
- Reports: sales pipeline, lead funnel and conversion, campaign performance, quotation register
- Assignees own their leads, deals and activities (own-records permissions)

        ## Install

        Workspace administrators install it from **Apps** in Ferrum; it ships in the official Ferrum
        server image. Self-hosters building their own image enable the `crm` feature of the server.
        Everything it adds is also available to AI agents over Ferrum's MCP server.

        ## Develop

        Clone it next to the platform and point the platform dependencies at your checkout (this file
        is git-ignored):

        ```toml
        # .cargo/config.toml
        [patch."https://github.com/CBFerrum/Ferrum"]
        ferrum-kernel = { path = "../ferrum/crates/kernel" }
        ferrum-meta = { path = "../ferrum/crates/meta" }
        ferrum-domain = { path = "../ferrum/crates/domain" }
        ```

        The tests need PostgreSQL with an ordinary (non-superuser) role — row-level security must apply:

        ```bash
        createuser ferrum --pwprompt && createdb -O ferrum ferrum_crm_test
        ```

        ```bash
        cargo test
        ```

        `FERRUM_CRM_TEST_DATABASE_URL` overrides the database URL. The app's metadata lives in `packages/` and is embedded
        at build time; `app.yaml` is its manifest (see Ferrum's `docs/plan/apps.md`).

        ## License

        AGPL-3.0-only, like the platform.
