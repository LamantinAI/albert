import { expect, test } from "vitest";
import { initialUnit, inUnit, nativeValue, settingUnits } from "./settingUnits";
test("readable units retain exact native values, including odd sizes and safe integer limits", () => {
  for (const value of [
    0,
    1,
    91,
    600,
    21600,
    33554432,
    Number.MAX_SAFE_INTEGER,
  ]) {
    for (const path of [
      "context.timeout_secs",
      "sessions.temporary_max_bytes",
    ]) {
      const units = settingUnits({ path }),
        selected = initialUnit(value, units);
      const text = inUnit(value, selected.factor)!;
      expect(nativeValue(text, selected.factor)).toBe(String(value));
    }
  }
  expect(
    initialUnit(21600, settingUnits({ path: "reflection.period_secs" })).label,
  ).toBe("Hours");
  expect(
    initialUnit(91, settingUnits({ path: "run.timeout_secs" })).label,
  ).toBe("Seconds");
  expect(nativeValue("1.25", 3600)).toBe("4500");
  expect(nativeValue(inUnit(1, 1073741824)!, 1073741824)).toBe("1");
});
test("fractional native values, overflow and bounds are rejected instead of rounded", () => {
  expect(() => nativeValue("0.1", 1)).toThrow();
  expect(() => nativeValue("1.0000000000000001", 60)).toThrow();
  expect(() => nativeValue("9007199254740992", 1)).toThrow();
  expect(() => nativeValue("-1", 1, 0)).toThrow();
  expect(() => nativeValue("2", 3600, 1, 3600)).toThrow();
  expect(nativeValue("0", 1, 0)).toBe("0");
});
