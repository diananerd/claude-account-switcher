<script setup lang="ts">
// The home page's first screen: what it is and how to install it on the left,
// the tool running in a terminal on the right (stacked on narrower screens).
// Text comes from the page's `intro` frontmatter.
import { useData } from "vitepress";
import { VPButton } from "vitepress/theme";
import InstallCommand from "./InstallCommand.vue";
import TerminalDemos from "./TerminalDemos.vue";

const { frontmatter } = useData();
</script>

<template>
  <section v-if="frontmatter.intro" class="hero">
    <div class="copy">
      <h1 class="name">{{ frontmatter.intro.name }}</h1>
      <!-- eslint-disable-next-line vue/no-v-html -- trusted page frontmatter -->
      <p class="tagline" v-html="frontmatter.intro.tagline" />
      <div class="actions">
        <VPButton
          v-for="a in frontmatter.intro.actions"
          :key="a.link"
          tag="a"
          size="medium"
          :theme="a.theme"
          :text="a.text"
          :href="a.link"
        />
      </div>
      <InstallCommand />
    </div>
    <div class="demo">
      <TerminalDemos />
    </div>
  </section>
</template>

<style scoped>
.hero {
  display: grid;
  grid-template-columns: minmax(0, 1fr);
  gap: 40px;
  max-width: 1280px;
  margin: 0 auto;
  padding: calc(var(--vp-nav-height) + 40px) 16px 56px;
}
@media (min-width: 640px) {
  .hero {
    gap: 40px;
    padding: calc(var(--vp-nav-height) + 40px) 48px 72px;
  }
}
@media (min-width: 1200px) {
  .hero {
    /* The left column fits the install command on one line. */
    grid-template-columns: minmax(500px, 5fr) minmax(0, 7fr);
    align-items: center;
    gap: 56px;
    padding: calc(var(--vp-nav-height) + 48px) 64px 80px;
  }
}
.copy {
  max-width: 620px;
}
.name {
  font-size: 36px;
  line-height: 1.12;
  font-weight: 700;
  letter-spacing: -0.02em;
  background: var(--vp-home-hero-name-background);
  -webkit-background-clip: text;
  background-clip: text;
  color: var(--vp-home-hero-name-color);
  /* Keeps the gradient text selectable and visible in forced-colors mode. */
  -webkit-text-fill-color: var(--vp-home-hero-name-color);
}
@media (min-width: 640px) {
  .name {
    font-size: 52px;
  }
}
@media (min-width: 1200px) {
  .name {
    font-size: 56px;
  }
}
.tagline {
  margin-top: 16px;
  font-size: 18px;
  line-height: 1.5;
  color: var(--vp-c-text-2);
}
@media (min-width: 640px) {
  .tagline {
    font-size: 21px;
  }
}
.tagline :deep(code) {
  font-size: 0.85em;
  padding: 2px 6px;
  border-radius: 4px;
  background: var(--vp-c-default-soft);
  color: var(--vp-c-text-1);
}
.actions {
  display: flex;
  flex-wrap: wrap;
  gap: 12px;
  margin-top: 28px;
}
@media (forced-colors: active) {
  .name {
    -webkit-text-fill-color: CanvasText;
  }
}
</style>
