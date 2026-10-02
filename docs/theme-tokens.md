# Theme tokens

The 40 editable tokens are declared in [theme.svelte.ts](../src/lib/utils/theme.svelte.ts). [Customization](../src/lib/settings/Customization.svelte) uses that registry for its editor; `applyTheme` sets or removes registered properties on the document root. [ThemeLayers](../src/lib/settings/ThemeLayers.svelte) applies the active theme and its CSS extensions. Theme CSS may override component rules with `!important`.

[The component checklist](theme-review.md#component-checklist) records every production Svelte component and its direct token references. Counts below are a source snapshot of those 95 components, including inline `var()` references. Inherited root styles and references in preset CSS are not counted. The checklist provides the reverse consumer mapping for every row, beyond the examples below.

## Defaults and custom themes

[The root stylesheet](../src/routes/+page.svelte) defines the default dark colors, font, radii and motion values. Its native `color-scheme` uses `--scheme` with a dark fallback. Dark, Light and Midnight supply 33 of 40 tokens and inherit the three radii, font family, font size, motion scale and easing. Glass supplies 38 and inherits font size and motion scale. Material supplies 39 and inherits motion scale. System selects the Dark or Light preset from the OS preference. The HTML shell now reads bg/text/font/scheme with fallbacks too, so body does not retain a hard-coded dark native-control scheme after theme application. This does not establish saved-theme cold first paint.

Imported themes may be partial. A missing property is cleared from the previous theme and falls back to the root default; a partial light custom theme therefore needs explicit light surface/text/scheme values. Duplicating a preset copies its values and CSS. Glass send-button and count-badge gradients now consume accent, hover and ink tokens, so editing the copy's accent affects those surfaces too. User CSS extensions can still intentionally override them. Reduced motion sets the effective motion scale to zero.

Native selects share raised/text/line-strong/radius-sm styles. Checkbox, radio and range accents share `--accent`. Native checkbox/radio/range focus retains the global focus outline; the text-input focus rule excludes them. [Panel](../src/lib/ui/Panel.svelte) also styles descendant `.field`, `.button` and `.switch` controls. These inherited rules do not prove that every component looks correct in every theme.

## surfaces

| Token | Use | Direct consumers | Examples |
| --- | --- | ---: | --- |
| `--bg` | Sidebar | 30 | [routes/+page.svelte](../src/routes/+page.svelte), [ui/Panel.svelte](../src/lib/ui/Panel.svelte), [messages/StarredList.svelte](../src/lib/messages/StarredList.svelte) |
| `--chat-bg` | Chat background | 8 | [routes/+page.svelte](../src/routes/+page.svelte), [messages/MessageBubble.svelte](../src/lib/messages/MessageBubble.svelte), [chat/ChatPreview.svelte](../src/lib/chat/ChatPreview.svelte) |
| `--surface` | Headers and bars | 48 | [routes/+page.svelte](../src/routes/+page.svelte), [ui/Panel.svelte](../src/lib/ui/Panel.svelte), [ui/ConfirmDialog.svelte](../src/lib/ui/ConfirmDialog.svelte) |
| `--raised` | Hover and inputs | 47 | [routes/+page.svelte](../src/routes/+page.svelte), [ui/Panel.svelte](../src/lib/ui/Panel.svelte), [ui/ConfirmDialog.svelte](../src/lib/ui/ConfirmDialog.svelte) |
| `--raised-2` | Selected | 23 | [routes/+page.svelte](../src/routes/+page.svelte), [ui/Panel.svelte](../src/lib/ui/Panel.svelte), [ui/Button.svelte](../src/lib/ui/Button.svelte) |
| `--scrim` | Dialog backdrop | 26 | [ui/Panel.svelte](../src/lib/ui/Panel.svelte), [ui/ConfirmDialog.svelte](../src/lib/ui/ConfirmDialog.svelte), [messages/StructuredNotice.svelte](../src/lib/messages/StructuredNotice.svelte) |
| `--shadow` | Popup shadow | 34 | [routes/+page.svelte](../src/routes/+page.svelte), [ui/Panel.svelte](../src/lib/ui/Panel.svelte), [ui/ConfirmDialog.svelte](../src/lib/ui/ConfirmDialog.svelte) |

## bubbles

| Token | Use | Direct consumers | Examples |
| --- | --- | ---: | --- |
| `--bubble` | Incoming bubble | 4 | [messages/MessageBubble.svelte](../src/lib/messages/MessageBubble.svelte), [messages/cards/PollCard.svelte](../src/lib/messages/cards/PollCard.svelte), [chat/ChatPreview.svelte](../src/lib/chat/ChatPreview.svelte) |
| `--bubble-mine` | Outgoing bubble | 4 | [messages/OutgoingItem.svelte](../src/lib/messages/OutgoingItem.svelte), [messages/MessageInfo.svelte](../src/lib/messages/MessageInfo.svelte), [messages/MessageBubble.svelte](../src/lib/messages/MessageBubble.svelte) |

## text

| Token | Use | Direct consumers | Examples |
| --- | --- | ---: | --- |
| `--text` | Text | 71 | [routes/+page.svelte](../src/routes/+page.svelte), [ui/Panel.svelte](../src/lib/ui/Panel.svelte), [ui/Logo.svelte](../src/lib/ui/Logo.svelte) |
| `--muted` | Secondary text | 77 | [routes/+page.svelte](../src/routes/+page.svelte), [ui/Panel.svelte](../src/lib/ui/Panel.svelte), [ui/Button.svelte](../src/lib/ui/Button.svelte) |
| `--faint` | Faint text | 18 | [routes/+page.svelte](../src/routes/+page.svelte), [ui/ConfirmDialog.svelte](../src/lib/ui/ConfirmDialog.svelte), [ui/Button.svelte](../src/lib/ui/Button.svelte) |
| `--link` | Links and read ticks | 11 | [messages/StructuredNotice.svelte](../src/lib/messages/StructuredNotice.svelte), [messages/MessageText.svelte](../src/lib/messages/MessageText.svelte), [messages/MessageInfo.svelte](../src/lib/messages/MessageInfo.svelte) |

## highlights

| Token | Use | Direct consumers | Examples |
| --- | --- | ---: | --- |
| `--mention` | Mentions of you: bar and text | 5 | [messages/StarredList.svelte](../src/lib/messages/StarredList.svelte), [messages/MessageText.svelte](../src/lib/messages/MessageText.svelte), [messages/MessageFinder.svelte](../src/lib/messages/MessageFinder.svelte) |
| `--mention-soft` | Mentions of you: row | 2 | [messages/MessageFinder.svelte](../src/lib/messages/MessageFinder.svelte), [messages/MessageBubble.svelte](../src/lib/messages/MessageBubble.svelte) |
| `--mention-self-soft` | Mentions of you: tag | 3 | [messages/MessageText.svelte](../src/lib/messages/MessageText.svelte), [messages/MessageFinder.svelte](../src/lib/messages/MessageFinder.svelte), [chat/GroupInfo.svelte](../src/lib/chat/GroupInfo.svelte) |
| `--mention-pill` | Mention tag text | 1 | [messages/MessageText.svelte](../src/lib/messages/MessageText.svelte) |
| `--mention-pill-soft` | Mention tag | 1 | [messages/MessageText.svelte](../src/lib/messages/MessageText.svelte) |
| `--replying` | Replying to: bar | 1 | [messages/MessageBubble.svelte](../src/lib/messages/MessageBubble.svelte) |
| `--replying-soft` | Replying to: row | 1 | [messages/MessageBubble.svelte](../src/lib/messages/MessageBubble.svelte) |
| `--jump-soft` | Jumped-to message | 1 | [messages/MessageBubble.svelte](../src/lib/messages/MessageBubble.svelte) |
| `--row-hover` | Message hover | 1 | [messages/MessageBubble.svelte](../src/lib/messages/MessageBubble.svelte) |

## accent

| Token | Use | Direct consumers | Examples |
| --- | --- | ---: | --- |
| `--accent` | Accent | 47 | [routes/+page.svelte](../src/routes/+page.svelte), [ui/Spinner.svelte](../src/lib/ui/Spinner.svelte), [ui/Panel.svelte](../src/lib/ui/Panel.svelte) |
| `--accent-hover` | Accent hover | 5 | [ui/Panel.svelte](../src/lib/ui/Panel.svelte), [ui/Button.svelte](../src/lib/ui/Button.svelte), [messages/cards/InviteCard.svelte](../src/lib/messages/cards/InviteCard.svelte) |
| `--accent-text` | Accent text | 17 | [routes/+page.svelte](../src/routes/+page.svelte), [composer/ComposerBar.svelte](../src/lib/composer/ComposerBar.svelte), [messages/MessageList.svelte](../src/lib/messages/MessageList.svelte) |
| `--accent-ink` | Text on accent | 13 | [ui/Panel.svelte](../src/lib/ui/Panel.svelte), [ui/Logo.svelte](../src/lib/ui/Logo.svelte), [ui/Button.svelte](../src/lib/ui/Button.svelte) |
| `--accent-soft` | Accent tint | 13 | [routes/+page.svelte](../src/routes/+page.svelte), [ui/Button.svelte](../src/lib/ui/Button.svelte), [messages/MessageMenuPanel.svelte](../src/lib/messages/MessageMenuPanel.svelte) |

## lines

| Token | Use | Direct consumers | Examples |
| --- | --- | ---: | --- |
| `--line` | Dividers | 37 | [ui/Panel.svelte](../src/lib/ui/Panel.svelte), [ui/BooleanProps.svelte](../src/lib/ui/BooleanProps.svelte), [composer/SlashCommandMenu.svelte](../src/lib/composer/SlashCommandMenu.svelte) |
| `--line-soft` | Soft dividers | 0 | Registered; no direct Svelte consumer. |
| `--line-strong` | Borders | 42 | [routes/+page.svelte](../src/routes/+page.svelte), [ui/Panel.svelte](../src/lib/ui/Panel.svelte), [ui/ConfirmDialog.svelte](../src/lib/ui/ConfirmDialog.svelte) |

## status

| Token | Use | Direct consumers | Examples |
| --- | --- | ---: | --- |
| `--danger` | Error | 57 | [routes/+page.svelte](../src/routes/+page.svelte), [ui/Panel.svelte](../src/lib/ui/Panel.svelte), [ui/ConfirmDialog.svelte](../src/lib/ui/ConfirmDialog.svelte) |
| `--danger-soft` | Error background | 4 | [routes/+page.svelte](../src/routes/+page.svelte), [messages/SelectionBar.svelte](../src/lib/messages/SelectionBar.svelte), [messages/MessageBubble.svelte](../src/lib/messages/MessageBubble.svelte) |

## shape and type

| Token | Use | Direct consumers | Examples |
| --- | --- | ---: | --- |
| `--radius-sm` | Bubble radius | 21 | [routes/+page.svelte](../src/routes/+page.svelte), [ui/Panel.svelte](../src/lib/ui/Panel.svelte), [ui/Button.svelte](../src/lib/ui/Button.svelte) |
| `--radius` | Input radius | 23 | [routes/+page.svelte](../src/routes/+page.svelte), [ui/Panel.svelte](../src/lib/ui/Panel.svelte), [ui/Button.svelte](../src/lib/ui/Button.svelte) |
| `--radius-lg` | Dialog radius | 27 | [ui/Panel.svelte](../src/lib/ui/Panel.svelte), [ui/ConfirmDialog.svelte](../src/lib/ui/ConfirmDialog.svelte), [messages/StructuredNotice.svelte](../src/lib/messages/StructuredNotice.svelte) |
| `--font` | Font family | 2 | [routes/+page.svelte](../src/routes/+page.svelte), [settings/ThemePreview.svelte](../src/lib/settings/ThemePreview.svelte) |
| `--font-size` | Font size | 3 | [routes/+page.svelte](../src/routes/+page.svelte), [chat/ChatPreview.svelte](../src/lib/chat/ChatPreview.svelte), [settings/ThemePreview.svelte](../src/lib/settings/ThemePreview.svelte) |
| `--scheme` | Native controls (dark/light) | 1 | [routes/+page.svelte](../src/routes/+page.svelte) |

## motion

| Token | Use | Direct consumers | Examples |
| --- | --- | ---: | --- |
| `--motion-scale` | Animation length (1 normal, 0 off, 2 slower) | 23 | [routes/+page.svelte](../src/routes/+page.svelte), [ui/Panel.svelte](../src/lib/ui/Panel.svelte), [ui/BooleanProps.svelte](../src/lib/ui/BooleanProps.svelte) |
| `--ease` | Easing curve | 18 | [routes/+page.svelte](../src/routes/+page.svelte), [ui/Panel.svelte](../src/lib/ui/Panel.svelte), [ui/BooleanProps.svelte](../src/lib/ui/BooleanProps.svelte) |

## Other CSS properties and color literals

Geometry and identity properties such as `--hue`, `--size`, `--fill`, `--pad-l`, `--pad-r`, `--embed-color`, `--played`, `--picker-h`, `--crop-max-height` and the customization swatch's `--c` are component data, not theme-editor tokens. `--line-soft` remains registered for compatibility but currently has no direct Svelte consumer.

Identity hues, QR white substrates and black/white media overlays serve different purposes from app surface colors. Their source exceptions are listed in the checklist, with visual contrast still pending. A literal inside a token fallback is inactive when the root supplies that token. Preset palettes and Glass wallpaper colors are theme definitions. Fixed shadow colors and identity luminance still need visual review in Light and custom themes.

## Focused source checks

```sh
node --experimental-strip-types --test src/lib/theme-coverage.test.ts
```

The seven checks cover source definitions/fallbacks, Glass accent references, documentation coverage, native-focus selector scope and leaf font/switch/alert rules and shared root, ChatPreview and TypingIndicator sender text, shell fallbacks and preview popup shadow. It does not calculate CSS inheritance, contrast, layout or native-control rendering. The separate [measured browser scope](theme-review.md#measured-synthetic-browser-scope) covers only synthetic native controls and the edited Glass send rule. See [the evidence status](theme-review.md#evidence-status) before claiming issue #220 acceptance.
