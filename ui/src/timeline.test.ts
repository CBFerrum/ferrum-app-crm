import { describe, expect, it } from "vitest";
import { horizon } from "./timeline";

describe("a day's work", () => {
  const now = new Date(2026, 10, 10, 15, 0);
  it("sorts what is due by the day it falls on, in the viewer's time", () => {
    expect(horizon(new Date(2026, 10, 9, 23, 0).toISOString(), now)).toBe("Overdue");
    expect(horizon(new Date(2026, 10, 10, 0, 30).toISOString(), now)).toBe("Today");
    expect(horizon(new Date(2026, 10, 10, 23, 30).toISOString(), now)).toBe("Today");
    expect(horizon(new Date(2026, 10, 17, 12, 0).toISOString(), now)).toBe("Next 7 days");
    expect(horizon(new Date(2026, 10, 18, 12, 0).toISOString(), now)).toBe("Later");
  });

  it("keeps what has no date, or a date it cannot read, apart", () => {
    expect(horizon(null, now)).toBe("No date");
    expect(horizon("not a date", now)).toBe("No date");
  });
});
