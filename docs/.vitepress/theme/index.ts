import DefaultTheme from "vitepress/theme";
import { h } from "vue";
import HomeHero from "./HomeHero.vue";
import "./custom.css";

export default {
  extends: DefaultTheme,
  Layout: () =>
    h(DefaultTheme.Layout, null, {
      "home-hero-before": () => h(HomeHero),
    }),
};
