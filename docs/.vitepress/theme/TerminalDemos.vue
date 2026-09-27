<script setup lang="ts">
// Terminal recordings of the real binary (scripts/record-demos.py), played with
// asciinema-player. Plays when scrolled into view, then moves on to the next
// scene; with reduced motion it shows each scene's last frame until asked.
import { onBeforeUnmount, onMounted, ref } from "vue";
import { withBase } from "vitepress";
import "asciinema-player/dist/bundle/asciinema-player.css";

const scenes = [
  { id: "use", label: "Use", summary: "Each folder runs claude with its own account; a new folder asks once." },
  { id: "switch", label: "Switch", summary: "Move a project to another account with csw; the next claude uses it." },
  { id: "setup", label: "Set up", summary: "Name your accounts, log them in and map folders, in five short steps." },
  { id: "install", label: "Install", summary: "The installer shows what it will change and asks first." },
] as const;

const active = ref(0);
const title = ref("zsh");
// Window titles the recorded shell set (OSC 0), with their times: the bar
// follows them as the demo plays, like a real terminal's.
let titles: { at: number; text: string }[] = [];
let ticker: ReturnType<typeof setInterval> | undefined;

async function loadTitles(url: string, playing: boolean) {
  titles = [];
  try {
    const lines = (await (await fetch(url)).text()).trim().split("\n").slice(1);
    // Sequences can span output chunks: search the whole stream, then map
    // each match back to the time of the chunk it starts in.
    let stream = "";
    const starts: { index: number; at: number }[] = [];
    for (const line of lines) {
      const [at, , data] = JSON.parse(line) as [number, string, string];
      starts.push({ index: stream.length, at });
      stream += data;
    }
    for (const m of stream.matchAll(/\u001b\]0;([^\u0007]*)\u0007/g)) {
      const at = starts.filter((s) => s.index <= (m.index ?? 0)).pop()?.at ?? 0;
      titles.push({ at, text: m[1] });
    }
  } catch {
    // No titles: the bar keeps the shell's name.
  }
  // Playing starts at the first title; the still poster shows the last frame.
  const shown = playing ? titles[0] : titles[titles.length - 1];
  title.value = shown?.text ?? "zsh";
}

async function syncTitle() {
  if (!player || !titles.length) return;
  const t = await player.getCurrentTime();
  if (typeof t !== "number" || Number.isNaN(t)) return;
  const last = titles.filter((x) => x.at <= t + 0.05).pop();
  if (last) title.value = last.text;
}
const screen = ref<HTMLElement | null>(null);
const frame = ref<HTMLElement | null>(null);
const paused = ref(false);
let player: any = null;
let lib: any = null;
let visible = false;
let started = false;
let next: ReturnType<typeof setTimeout> | undefined;
let observer: IntersectionObserver | undefined;

const reduced = () => window.matchMedia("(prefers-reduced-motion: reduce)").matches;
// Below this width, fitting 80 columns would shrink the text past reading.
const narrow = () => (frame.value?.clientWidth ?? 800) < 560;

function mount(play: boolean) {
  clearTimeout(next);
  player?.dispose();
  if (!lib || !screen.value) return;
  const scene = scenes[active.value];
  const url = withBase(`/demos/${scene.id}.cast`);
  loadTitles(url, play);
  clearInterval(ticker);
  ticker = setInterval(syncTitle, 200);
  player = lib.create(url, screen.value, {
    autoPlay: play,
    loop: false,
    idleTimeLimit: 1.5,
    poster: play ? undefined : "npt:1:00",
    controls: false,
    fit: narrow() ? false : "width",
    terminalFontSize: narrow() ? "11px" : undefined,
    terminalFontFamily: "var(--vp-font-family-mono)",
    theme: "csw",
  });
  paused.value = !play;
  player.addEventListener("ended", () => {
    paused.value = true;
    if (reduced()) return;
    next = setTimeout(() => select((active.value + 1) % scenes.length, true), 3500);
  });
}

function select(i: number, play = !reduced()) {
  active.value = i;
  mount(play && visible);
}

function toggle() {
  if (!player) return;
  if (paused.value) {
    player.play();
    paused.value = false;
  } else {
    player.pause();
    paused.value = true;
  }
}

function onKey(e: KeyboardEvent, i: number) {
  const n = scenes.length;
  const to = e.key === "ArrowRight" ? (i + 1) % n : e.key === "ArrowLeft" ? (i + n - 1) % n : -1;
  if (to < 0) return;
  e.preventDefault();
  select(to);
  (document.getElementById(`demo-tab-${scenes[to].id}`) as HTMLElement | null)?.focus();
}

onMounted(async () => {
  lib = await import("asciinema-player");
  mount(false);
  observer = new IntersectionObserver(
    ([entry]) => {
      visible = entry.isIntersecting;
      if (visible && !started && !reduced()) {
        started = true;
        mount(true);
      } else if (!visible && player && !paused.value) {
        player.pause();
        paused.value = true;
      }
    },
    { threshold: 0.4 },
  );
  if (frame.value) observer.observe(frame.value);
});

onBeforeUnmount(() => {
  clearTimeout(next);
  clearInterval(ticker);
  observer?.disconnect();
  player?.dispose();
});

</script>

<template>
  <section class="demos" aria-labelledby="demos-heading">
    <h2 id="demos-heading" class="sr-only">See it in a terminal</h2>
    <div class="tabs" role="tablist" aria-label="Terminal demos">
      <button
        v-for="(s, i) in scenes"
        :id="`demo-tab-${s.id}`"
        :key="s.id"
        type="button"
        role="tab"
        :aria-selected="active === i"
        aria-controls="demo-panel"
        :tabindex="active === i ? 0 : -1"
        :class="{ on: active === i }"
        @click="select(i)"
        @keydown="onKey($event, i)"
      >
        {{ s.label }}
      </button>
    </div>
    <div id="demo-panel" ref="frame" class="window" role="tabpanel" :aria-labelledby="`demo-tab-${scenes[active].id}`">
      <div class="bar">
        <span class="dots" aria-hidden="true"><i /><i /><i /></span>
        <span class="title">{{ title }}</span>
        <button type="button" class="play" :aria-label="paused ? 'Play the demo' : 'Pause the demo'" @click="toggle">
          <svg v-if="paused" viewBox="0 0 16 16" aria-hidden="true"><path d="M4 2.5v11l9-5.5z" /></svg>
          <svg v-else viewBox="0 0 16 16" aria-hidden="true"><path d="M4 2.5h3v11H4zM9 2.5h3v11H9z" /></svg>
        </button>
      </div>
      <div ref="screen" class="screen" />
    </div>
    <p class="caption">
      {{ scenes[active].summary }}
      <span class="note">Recorded from the real <code>claude-switcher</code> with example accounts; Claude Code itself is a stand-in.</span>
    </p>
  </section>
</template>

<style scoped>
.demos {
  min-width: 0;
}
.tabs {
  display: flex;
  gap: 4px;
  margin-bottom: 12px;
}
.tabs button {
  padding: 6px 14px;
  border-radius: 999px;
  font-size: 14px;
  font-weight: 500;
  color: var(--vp-c-text-2);
  transition: color 0.2s, background-color 0.2s;
}
.tabs button:hover {
  color: var(--vp-c-text-1);
}
.tabs button.on {
  color: var(--vp-c-brand-1);
  background: var(--vp-c-brand-soft);
}
.tabs button:focus-visible,
.play:focus-visible {
  outline: 2px solid var(--vp-c-brand-1);
  outline-offset: 2px;
}
.window {
  overflow: hidden;
  border-radius: 12px;
  border: 1px solid #2b2724;
  background: #1c1917;
  box-shadow: 0 30px 70px -30px rgba(28, 25, 23, 0.5), 0 8px 20px -12px rgba(28, 25, 23, 0.3);
}
:global(.dark) .window {
  border-color: #3a3431;
  box-shadow: 0 30px 70px -30px rgba(0, 0, 0, 0.8);
}
.bar {
  display: flex;
  align-items: center;
  gap: 12px;
  height: 38px;
  padding: 0 10px 0 14px;
  background: #262220;
  border-bottom: 1px solid #332e2b;
}
.dots {
  display: flex;
  gap: 7px;
}
.dots i {
  width: 11px;
  height: 11px;
  border-radius: 50%;
  background: #4a4340;
}
.title {
  flex: 1;
  text-align: center;
  font-family: var(--vp-font-family-mono);
  font-size: 12px;
  color: #a8a29e;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.play {
  display: grid;
  place-items: center;
  width: 28px;
  height: 28px;
  border-radius: 6px;
  color: #d6d3d1;
}
.play:hover {
  background: #332e2b;
}
.play svg {
  width: 14px;
  height: 14px;
  fill: currentColor;
}
.screen {
  min-height: 240px;
  padding: 10px 6px 12px;
  overflow-x: auto;
}
.caption {
  margin-top: 12px;
  font-size: 14px;
  line-height: 1.6;
  color: var(--vp-c-text-2);
}
.caption .note {
  display: block;
  font-size: 13px;
  color: var(--vp-c-text-3);
}
.caption code {
  font-size: 12px;
}
.sr-only {
  position: absolute;
  width: 1px;
  height: 1px;
  overflow: hidden;
  clip: rect(0 0 0 0);
  white-space: nowrap;
}
</style>

<style>
/* Terminal palette for the demos: warm dark, same family as the site accent. */
.asciinema-player-theme-csw {
  --term-color-foreground: #e7e5e4;
  --term-color-background: #1c1917;
  --term-color-0: #292524;
  --term-color-1: #f87171;
  --term-color-2: #86c28b;
  --term-color-3: #e8b86c;
  --term-color-4: #7aa7e0;
  --term-color-5: #c79be0;
  --term-color-6: #6cc4c4;
  --term-color-7: #d6d3d1;
  --term-color-8: #78716c;
  --term-color-9: #fca5a5;
  --term-color-10: #a7d7ab;
  --term-color-11: #f0cd8f;
  --term-color-12: #a3c3ec;
  --term-color-13: #dab8ec;
  --term-color-14: #99dada;
  --term-color-15: #fafaf9;
}
.demos .ap-wrapper {
  justify-content: flex-start;
}
.demos .ap-player {
  border-radius: 0;
}
</style>
