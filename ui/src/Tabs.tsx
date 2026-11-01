// CRM's own screens, at the top of each.

import { NavLink } from "react-router";

export function Tabs() {
  return (
    <nav className="crm-tabs" aria-label="CRM">
      <NavLink to="/" end>
        Pipeline
      </NavLink>
      <NavLink to="/today">Today</NavLink>
    </nav>
  );
}
