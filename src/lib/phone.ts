// ITU calling codes. They are prefix-free, so the longest match is the country.
// +1 is shared by 25 countries (NANP); telling them apart needs the
// area code table, so +1 is shown as the code itself.
const CODES: Record<string, string> = Object.fromEntries(
  (
    "7:RU,20:EG,27:ZA,30:GR,31:NL,32:BE,33:FR,34:ES,36:HU,39:IT,40:RO,41:CH,43:AT,44:GB," +
    "45:DK,46:SE,47:NO,48:PL,49:DE,51:PE,52:MX,53:CU,54:AR,55:BR,56:CL,57:CO,58:VE,60:MY," +
    "61:AU,62:ID,63:PH,64:NZ,65:SG,66:TH,81:JP,82:KR,84:VN,86:CN,90:TR,91:IN,92:PK,93:AF," +
    "94:LK,95:MM,98:IR,211:SS,212:MA,213:DZ,216:TN,218:LY,220:GM,221:SN,222:MR,223:ML," +
    "224:GN,225:CI,226:BF,227:NE,228:TG,229:BJ,230:MU,231:LR,232:SL,233:GH,234:NG,235:TD," +
    "236:CF,237:CM,238:CV,239:ST,240:GQ,241:GA,242:CG,243:CD,244:AO,245:GW,246:IO,248:SC," +
    "249:SD,250:RW,251:ET,252:SO,253:DJ,254:KE,255:TZ,256:UG,257:BI,258:MZ,260:ZM,261:MG," +
    "262:RE,263:ZW,264:NA,265:MW,266:LS,267:BW,268:SZ,269:KM,290:SH,291:ER,297:AW,298:FO," +
    "299:GL,350:GI,351:PT,352:LU,353:IE,354:IS,355:AL,356:MT,357:CY,358:FI,359:BG,370:LT," +
    "371:LV,372:EE,373:MD,374:AM,375:BY,376:AD,377:MC,378:SM,380:UA,381:RS,382:ME,383:XK," +
    "385:HR,386:SI,387:BA,389:MK,420:CZ,421:SK,423:LI,500:FK,501:BZ,502:GT,503:SV,504:HN," +
    "505:NI,506:CR,507:PA,508:PM,509:HT,590:GP,591:BO,592:GY,593:EC,594:GF,595:PY,596:MQ," +
    "597:SR,598:UY,599:CW,670:TL,672:NF,673:BN,674:NR,675:PG,676:TO,677:SB,678:VU,679:FJ," +
    "680:PW,681:WF,682:CK,683:NU,685:WS,686:KI,687:NC,688:TV,689:PF,690:TK,691:FM,692:MH," +
    "850:KP,852:HK,853:MO,855:KH,856:LA,880:BD,886:TW,960:MV,961:LB,962:JO,963:SY,964:IQ," +
    "965:KW,966:SA,967:YE,968:OM,970:PS,971:AE,972:IL,973:BH,974:QA,975:BT,976:MN,977:NP," +
    "992:TJ,993:TM,994:AZ,995:GE,996:KG,998:UZ"
  )
    .split(",")
    .map((pair) => pair.split(":")),
);

/** The flag emoji for a two-letter country code. */
function flag(iso: string) {
  return String.fromCodePoint(...[...iso].map((c) => 0x1f1e6 + c.charCodeAt(0) - 65));
}

/**
 * `59891954564` → `🇺🇾 +598 91954564`: the country as a flag and its calling
 * code, then the national number. `null` when the digits cannot be placed.
 */
export function phoneLabel(digits: string): string | null {
  if (!/^\d{7,15}$/.test(digits)) return null;
  if (digits.startsWith("1")) return `+1 ${digits.slice(1)}`;
  for (const length of [3, 2, 1]) {
    let iso = CODES[digits.slice(0, length)];
    if (!iso) continue;
    // Kazakhstan shares +7 with Russia, on the 6xx and 7xx ranges.
    if (iso === "RU" && /^7[67]/.test(digits)) iso = "KZ";
    return `${flag(iso)} +${digits.slice(0, length)} ${digits.slice(length)}`;
  }
  return null;
}

/** A number standing in for a name: bare digits, or a `+` label such as WhatsApp's masked `+598∙∙∙∙∙27`. */
export function isPlaceholder(name: string): boolean {
  return /^\+?\d+$/.test(name) || (name.startsWith("+") && !/\p{L}/u.test(name));
}

/**
 * A name for someone, falling back to their number with its country when no
 * name is known. Only phone JIDs carry a number; a LID is left as it is.
 */
export function displayName(name: string | null | undefined, jid: string): string {
  if (name && !isPlaceholder(name)) return name;
  const user = jid.split("@")[0].split(":")[0];
  const digits = name?.replace("+", "") ?? (jid.endsWith("@s.whatsapp.net") ? user : null);
  return (digits && phoneLabel(digits)) || name || user;
}

// Self-check, run with `node --experimental-strip-types src/lib/phone.ts`.
const argv = (globalThis as { process?: { argv?: string[] } }).process?.argv;
if (argv?.[1]?.endsWith("phone.ts")) {
  const assert = (ok: boolean, what: string) => {
    if (!ok) throw new Error(what);
  };
  assert(phoneLabel("59891954564") === `${flag("UY")} +598 91954564`, "Uruguay");
  assert(phoneLabel("447911123456") === `${flag("GB")} +44 7911123456`, "UK");
  assert(phoneLabel("77011234567") === `${flag("KZ")} +7 7011234567`, "Kazakhstan");
  assert(phoneLabel("12025550123") === "+1 2025550123", "NANP");
  assert(displayName("Joaquin", "598@s.whatsapp.net") === "Joaquin", "name wins");
  assert(displayName(null, "138947158093828@lid") === "138947158093828", "LID untouched");
  assert(displayName(null, "59891954564:3@s.whatsapp.net") === `${flag("UY")} +598 91954564`, "device suffix");
  console.log("phone.ts ok");
}
