/**
 * How a number written by a person is read into a number box: digits in
 * Arabic-Indic or Persian as digits, and a comma as whichever mark it
 * must be. A comma before exactly three digits, or one of several, is a
 * thousands mark ("1,920" is 1920, "1,920,000" a million) — unless the
 * browser's own language writes its decimals with a comma, where it is
 * always the decimal. Any other comma is a decimal ("12,5" is 12.5).
 * With both a comma and a point, the later of them is the decimal
 * ("1.234,5" and "1,234.5" are both 1234.5).
 */
export function plainDigits(text: string): string {
  return text
    .replace(/[٠-٩]/g, (d) => String(d.charCodeAt(0) - 0x0660))
    .replace(/[۰-۹]/g, (d) => String(d.charCodeAt(0) - 0x06f0));
}

/** Whether the browser's language writes 1.5 as "1,5". */
export function commaIsDecimal(locale?: string): boolean {
  try {
    return (1.5).toLocaleString(locale).includes(",");
  } catch {
    return false;
  }
}

/** Whole text written elsewhere (pasted, dropped), as a number box takes it. */
export function readWritten(text: string, commaDecimal: boolean): string {
  const t = plainDigits(text.trim());
  const comma = t.lastIndexOf(",");
  const point = t.lastIndexOf(".");
  if (comma < 0) return t;
  if (point >= 0) {
    return comma > point ? t.replace(/\./g, "").replace(",", ".") : t.replace(/,/g, "");
  }
  const commas = t.split(",").length - 1;
  if (!commaDecimal && (commas > 1 || /^[-+]?\d{1,3},\d{3}$/.test(t))) return t.replace(/,/g, "");
  if (commas > 1) return t.replace(/,/g, "");
  return t.replace(",", ".");
}

/**
 * What a box holds when the caret leaves it, after commas typed into it
 * were taken as points on the way (a box of numbers cannot hold a
 * comma): `commas` of them, and `point` if a point was typed too. Two
 * or more, or one before exactly three digits, were thousands marks.
 */
export function settleTyped(value: string, commas: number, point: boolean, commaDecimal: boolean): string {
  if (commas === 0 || point || commaDecimal) return value;
  if (commas > 1 || /^[-+]?\d{1,3}\.\d{3}$/.test(value)) return value.replace(/\./g, "");
  return value;
}
