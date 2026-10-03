import assert from "node:assert/strict";
import test from "node:test";
import { englishCatalog, installLocaleProvider } from "../lib/i18n/localizer.ts";
import { mediaClock } from "../lib/media/clock.ts";

test("media clock follows locale switches without changing duration or hour mode", () => {
  let locale = "en";
  const restore = installLocaleProvider(() => ({ locale, catalog: englishCatalog }));
  try {
    assert.equal(mediaClock(69), "1:09");
    assert.equal(mediaClock(3601), "60:01");
    assert.equal(mediaClock(3601, true), "1:00:01");
    locale = "ar-EG-u-nu-arab";
    assert.equal(mediaClock(69), "١:٠٩");
    assert.equal(mediaClock(3601, true), "١:٠٠:٠١");
    for (const invalid of [NaN, Infinity, -1]) assert.equal(mediaClock(invalid), "٠:٠٠");
  } finally { restore(); }
});
