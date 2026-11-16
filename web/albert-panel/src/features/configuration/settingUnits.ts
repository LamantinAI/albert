import type { ConfigField } from "./useStructuredDraft";
export type Unit = { value: string; label: string; factor: number };
const seconds = [
  { value: "seconds", label: "Seconds", factor: 1 },
  { value: "minutes", label: "Minutes", factor: 60 },
  { value: "hours", label: "Hours", factor: 3600 },
];
const bytes = [
  { value: "bytes", label: "Bytes", factor: 1 },
  { value: "kib", label: "KiB", factor: 1024 },
  { value: "mib", label: "MiB", factor: 1048576 },
  { value: "gib", label: "GiB", factor: 1073741824 },
];
export function settingUnits(field: Pick<ConfigField, "path">): Unit[] {
  return field.path.endsWith("_secs")
    ? seconds
    : field.path.endsWith("_bytes")
      ? bytes
      : [];
}
export function initialUnit(value: number, units: Unit[]) {
  return (
    [...units]
      .reverse()
      .find((unit) => value > 0 && value % unit.factor === 0) ?? units[0]
  );
}
export function inUnit(value: number, factor: number): string | null {
  if (!Number.isSafeInteger(value)) return null;
  const negative = value < 0,
    amount = BigInt(Math.abs(value)),
    divisor = BigInt(factor);
  let result = String(amount / divisor),
    remainder = amount % divisor;
  if (remainder) {
    result += ".";
    for (let i = 0; remainder && i < 64; i++) {
      remainder *= 10n;
      result += String(remainder / divisor);
      remainder %= divisor;
    }
  }
  return remainder ? null : `${negative ? "-" : ""}${result}`;
}
export function nativeValue(
  text: string,
  factor: number,
  minimum?: number | null,
  maximum?: number | null,
): string {
  if (text.length > 128) throw new Error("Value is too long");
  const match = /^([+-]?)(\d*)(?:\.(\d*))?$/.exec(text.trim());
  if (!match || !(match[2] || match[3])) throw new Error("Enter a number");
  const decimals = match[3] ?? "";
  const denominator = 10n ** BigInt(decimals.length);
  let numerator = BigInt((match[2] || "0") + decimals) * BigInt(factor);
  if (match[1] === "-") numerator = -numerator;
  if (numerator % denominator)
    throw new Error("Use an exact whole number of the base unit");
  const result = numerator / denominator;
  if (
    result > BigInt(Number.MAX_SAFE_INTEGER) ||
    result < BigInt(Number.MIN_SAFE_INTEGER)
  )
    throw new Error("Value is too large");
  const number = Number(result);
  if (minimum != null && number < minimum)
    throw new Error(`Minimum: ${minimum.toLocaleString()}`);
  if (maximum != null && number > maximum)
    throw new Error(`Maximum: ${maximum.toLocaleString()}`);
  return String(result);
}
