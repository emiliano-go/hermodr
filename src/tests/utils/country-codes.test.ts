import assert from "node:assert/strict";
import test from "node:test";
import { COUNTRIES, composeE164, defaultCountryIso } from "../../lib/utils/country-codes.ts";

test("phone numbers are dial prefix plus national digits without the trunk zero", () => {
  assert.equal(composeE164("598", "099 123 456"), "59899123456");
  assert.equal(composeE164("1", "(555) 123-4567"), "15551234567");
  // Not enough to dial: missing prefix or under seven digits.
  assert.equal(composeE164("", "99123456"), null);
  assert.equal(composeE164("598", "123"), null);
});

test("the locale picks the preselected country when it is known", () => {
  assert.equal(defaultCountryIso("es-UY"), "UY");
  assert.equal(defaultCountryIso("en-US"), "US");
  assert.equal(defaultCountryIso("not-a-locale"), "");
});

test("the picker lists every dial code with a name, sorted by name", () => {
  assert.equal(COUNTRIES.find((country) => country.iso === "UY")?.dial, "598");
  assert.ok(COUNTRIES.length > 200);
  const names = COUNTRIES.map((country) => country.name);
  assert.deepEqual(names, [...names].sort((a, b) => a.localeCompare(b)));
});
