// CRM's own screens, at the top of each.

import { NavLink } from "react-router";
import { t } from "@ferrum/client";

export function Tabs() {
  return (
    <nav className="crm-tabs" aria-label={t("CRM")}>
      <NavLink to="/" end>
        {t("Pipeline")}
      </NavLink>
      <NavLink to="/today">{t("Today")}</NavLink>
      <NavLink to="/forecast">{t("Forecast")}</NavLink>
    </nav>
  );
}
