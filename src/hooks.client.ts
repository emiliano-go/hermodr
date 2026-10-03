import { locale } from "$lib/i18n/locale.svelte";

export async function init() {
  await locale.init();
}
