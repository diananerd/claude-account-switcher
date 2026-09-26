<script setup lang="ts">
import { ref } from "vue";

const command = "curl -fsSL https://switcher.diananerd.com | sh";
const copied = ref(false);

const code = ref<HTMLElement | null>(null);

async function copy() {
  try {
    await navigator.clipboard.writeText(command);
    copied.value = true;
    setTimeout(() => (copied.value = false), 1600);
  } catch {
    // Clipboard blocked: select the command so it can be copied by hand.
    const range = document.createRange();
    if (code.value) range.selectNodeContents(code.value);
    getSelection()?.removeAllRanges();
    getSelection()?.addRange(range);
  }
}
</script>

<template>
  <div class="install">
    <div class="line">
      <code><span class="prompt" aria-hidden="true">$</span><span ref="code">{{ command }}</span></code>
      <button type="button" :aria-label="copied ? 'Copied' : 'Copy the install command'" @click="copy">
        {{ copied ? "Copied" : "Copy" }}
      </button>
    </div>
    <p class="note">Shows what it will change and asks first. macOS and Linux; tested on macOS.</p>
    <span class="sr-only" aria-live="polite">{{ copied ? "Install command copied" : "" }}</span>
  </div>
</template>

<style scoped>
.install {
  margin-top: 28px;
  max-width: 560px;
}
.line {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 10px 10px 10px 16px;
  border: 1px solid var(--vp-c-divider);
  border-radius: 10px;
  background: var(--vp-c-bg-soft);
}
code {
  flex: 1;
  min-width: 0;
  overflow-x: auto;
  white-space: nowrap;
  font-family: var(--vp-font-family-mono);
  font-size: 14px;
  color: var(--vp-c-text-1);
}
.prompt {
  margin-right: 10px;
  color: var(--vp-c-text-3);
  user-select: none;
}
button {
  flex: none;
  padding: 6px 12px;
  border-radius: 6px;
  font-size: 13px;
  font-weight: 600;
  color: var(--vp-button-brand-text);
  background: var(--vp-button-brand-bg);
}
button:hover {
  background: var(--vp-button-brand-hover-bg);
}
button:focus-visible {
  outline: 2px solid var(--vp-c-brand-1);
  outline-offset: 2px;
}
.note {
  margin-top: 8px;
  font-size: 13px;
  color: var(--vp-c-text-2);
}
.sr-only {
  position: absolute;
  width: 1px;
  height: 1px;
  overflow: hidden;
  clip: rect(0 0 0 0);
}
</style>
