<!-- The three bouncing dots of a typing indicator. -->
<script lang="ts">
  let { hue: tint, label = "typing" }: { hue?: number; label?: string } = $props();
</script>

<span class="dots" aria-label={label} style={tint !== undefined ? `--hue: ${tint}` : null}
  ><i></i><i></i><i></i></span
>

<style>
  .dots {
    display: flex;
    gap: 4px;
    padding: 4px 2px;
  }
  .dots i {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: hsl(var(--hue, 160) 65% 68%);
    animation: blink 1.2s infinite ease-in-out;
  }
  .dots i:nth-child(2) {
    animation-delay: calc(0.15s * var(--motion-scale, 1));
  }
  .dots i:nth-child(3) {
    animation-delay: calc(0.3s * var(--motion-scale, 1));
  }
  @keyframes blink {
    0%,
    60%,
    100% {
      opacity: 0.35;
      transform: translateY(0);
    }
    30% {
      opacity: 1;
      transform: translateY(-3px);
    }
  }
</style>
