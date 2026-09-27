<script lang="ts">
  import Button from "$lib/Button.svelte";
  import ChatHeader from "$lib/ChatHeader.svelte";
  import ChatSidebar from "$lib/ChatSidebar.svelte";
  import ComposerBar from "$lib/ComposerBar.svelte";
  import ConfirmDialog from "$lib/ConfirmDialog.svelte";
  import MessageBubble from "$lib/MessageBubble.svelte";
  import MessageMenuPanel from "$lib/MessageMenuPanel.svelte";
  import PairingView from "$lib/PairingView.svelte";
  import { account, bubbleApi, bubbleView, chats, menuItems, messages, noop, rowTime, selectedChat } from "$lib/theme-preview";

  export type Scene = "chat" | "signin" | "menu" | "dialog";
  let { scene }: { scene: Scene } = $props();
  const STAGE_W = 1000;
  const STAGE_H = 600;
  let width = $state(STAGE_W);
  const zoom = $derived(width / STAGE_W);
</script>

<div class="frame" bind:clientWidth={width} style:height="{STAGE_H * zoom}px" aria-hidden="true" inert>
  <div class="stage" style:zoom style:width="{STAGE_W}px" style:height="{STAGE_H}px">
    {#if scene === "signin"}
      <PairingView
        qrSvg={null} started={true} connecting={false} connected={true}
        choosingAccount={false} accounts={[account]} activeAccount={account.id}
        accountAvatars={{}} linked={account} finalizing={false}
        syncPending={400} syncApplied={120} syncPercent={30} syncTimedOut={false}
        onconnect={noop} onchoose={noop} onswitch={noop} onsettings={noop}
      />
    {:else}
      <div class="layout">
        <ChatSidebar
          searchQuery="" searchResults={[]} visibleChats={chats} {selectedChat}
          chatFilter="all" onfilter={noop} unreadChats={2} unreadPings={1}
          avatars={{}} chatLabelOf={(chat) => chat.display_name ?? ""}
          formatTime={rowTime} typingLabelOf={() => null} previewAuthorOf={() => null}
          previewTextOf={(chat) => chat.last_text} mediaIconOf={() => null} groupKinds={{}}
          accounts={[account]} activeAccount={account.id} activeLabel={account.label}
          accountAvatars={{}} me={account.jid} meVersion={0} visibility="online" accountMenu={false}
          onmenutoggle={noop} onswitchaccount={noop} onaddaccount={noop} onsettings={noop}
          onpings={noop} onstarred={noop} onsearch={noop} onopenresult={noop} onopenchat={noop}
          ontogglepin={noop} onclearchat={noop} ondeletechat={noop} onchataction={noop}
          onmarkread={noop} archivedChats={0} keepingOnce={false} onkeeponce={noop} onresize={noop}
        />
        <section class="conversation">
          <ChatHeader
            {selectedChat} isGroup={true} title="Design team" avatar={null}
            typingNow={null} subtitle="Ana, Diego, Laura, You" groupContext={null} presenceText={null}
            mentionTotal={1} mentionCursor={0} pinned={null}
            ongroupinfo={noop} onsearch={noop} onpings={noop} onsettings={noop}
            onjumpmention={noop} onpinnedjump={noop} onclearchat={noop} ondeletechat={noop}
          />
          <div class="messages group">
            {#each messages as message, index (message.id)}
              <MessageBubble {message} vm={bubbleView(message, index, scene)} api={bubbleApi} />
            {/each}
          </div>
          <ComposerBar
            draft="" composerInput={undefined} replyingTo={scene === "chat" ? messages[1] : null}
            replyAuthor="Ana" replySnippet={messages[1].text} editing={null} pending={[]}
            recording={false} mentionMatches={[]} mentionIndex={0} onselectmention={noop}
            emojiToken={null} emojiMatches={[]} emojiIndex={0} onselectemoji={noop} pickerTab={null}
            {selectedChat} enqueue={(task) => task()} takereply={() => ({})}
            onpickeremoji={noop} onpickersent={noop} onpickererror={noop} onstage={noop}
            oncreatekind={noop} oninput={noop} onkey={noop} onsend={noop}
            oncancelreply={noop} oncanceledit={noop} onremove={noop} ontoggleonce={noop} onsendvoice={noop}
            onvoiceerror={noop} onreceipts={noop} ontyping={noop}
            receiptsHidden={false} typingHidden={false}
          />
        </section>
      </div>
      {#if scene === "menu"}
        <MessageMenuPanel left={640} top={180} items={menuItems}
          reactions={["👍", "❤️", "😂", "😮", "😢", "🙏"]} current="❤️" onreact={noop} onclose={noop} />
      {:else if scene === "dialog"}
        <ConfirmDialog label="Delete message" title="Delete message?"
          hint="Delete it for everyone in this chat, or only from your devices." onclose={noop}>
          {#snippet actions()}
            <Button variant="ghost" danger onclick={noop}>Delete for everyone</Button>
            <Button variant="ghost" danger onclick={noop}>Delete for me</Button>
            <Button variant="ghost" onclick={noop}>Cancel</Button>
          {/snippet}
        </ConfirmDialog>
      {/if}
    {/if}
  </div>
</div>

<style>
  .frame {
    position: relative;
    overflow: hidden;
    border: 1px solid var(--line-strong);
    border-radius: var(--radius-lg);
    background: var(--bg);
  }
  .stage {
    position: relative;
    transform: translateZ(0);
    overflow: hidden;
    background: var(--bg);
    color: var(--text);
    font-family: "Twemoji Country Flags", var(--font);
    font-size: var(--font-size);
    line-height: normal;
    -webkit-font-smoothing: antialiased;
    user-select: none;
    pointer-events: none;
  }
  .stage :global(*) { transition: none !important; }
  .layout { display: grid; grid-template-columns: 300px 1fr; height: 100%; overflow: hidden; }
  .conversation {
    display: flex; flex-direction: column; min-height: 0; min-width: 0;
    position: relative; background: var(--chat-bg);
  }
  .messages {
    flex: 1; overflow: hidden; min-width: 0; padding: 12px 0 8px;
    display: flex; flex-direction: column; gap: 2px;
    --pad-l: max(56px, 7%); --pad-r: clamp(16px, 7%, 90px);
  }
</style>
