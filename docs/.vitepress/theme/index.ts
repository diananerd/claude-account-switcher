import DefaultTheme from "vitepress/theme";
import { h } from "vue";
import InstallCommand from "./InstallCommand.vue";
import "./custom.css";

export default {
  extends: DefaultTheme,
  Layout: () => h(DefaultTheme.Layout, null, { "home-hero-actions-after": () => h(InstallCommand) }),
};
