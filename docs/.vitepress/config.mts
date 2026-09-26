import { readFileSync } from "node:fs";
import { defineConfig } from "vitepress";

// The version shown in the nav comes from the crate, so it never goes stale.
const version = readFileSync(new URL("../../Cargo.toml", import.meta.url), "utf8").match(/^version = "(.+)"/m)?.[1];

const repo = "https://github.com/diananerd/claude-account-switcher";

// README.md is written for GitHub; on this site its repository links point to
// pages or to GitHub.
const links: Record<string, string> = {
  "docs/reference.md": "/reference",
  "docs/comparison.md": "/comparison",
  "docs/how-it-works.md": "/how-it-works",
  "AGENTS.md": `${repo}/blob/main/AGENTS.md`,
  LICENSE: `${repo}/blob/main/LICENSE`,
};

export default defineConfig({
  title: "Claude Account Switcher",
  description: "Run several Claude Code accounts on one machine, chosen per folder.",
  cleanUrls: true,
  lastUpdated: true,
  srcExclude: ["README.md"],
  sitemap: { hostname: "https://switcher.diananerd.com" },
  head: [
    ["link", { rel: "icon", type: "image/svg+xml", href: "/logo.svg" }],
    ["meta", { name: "theme-color", content: "#b4532e" }],
    ["meta", { property: "og:title", content: "Claude Account Switcher" }],
    ["meta", { property: "og:description", content: "Run several Claude Code accounts on one machine, chosen per folder." }],
  ],
  markdown: {
    config(md) {
      md.core.ruler.after("inline", "repo-links", (state) => {
        for (const token of state.tokens) {
          for (const child of token.children ?? []) {
            const href = child.type === "link_open" ? child.attrGet("href") : null;
            if (href && links[href]) child.attrSet("href", links[href]);
          }
        }
      });
    },
  },
  themeConfig: {
    logo: "/logo.svg",
    nav: [
      { text: "Guide", link: "/guide" },
      { text: "Reference", link: "/reference" },
      { text: "Comparison", link: "/comparison" },
      { text: `v${version}`, items: [
        { text: "Changelog", link: `${repo}/blob/main/CHANGELOG.md` },
        { text: "Releases", link: `${repo}/releases` },
      ] },
    ],
    sidebar: [
      {
        text: "Docs",
        items: [
          { text: "Guide", link: "/guide" },
          { text: "Reference", link: "/reference" },
          { text: "How it works", link: "/how-it-works" },
          { text: "Comparison", link: "/comparison" },
          { text: "For LLMs (llms.txt)", link: "/llms.txt", target: "_self" },
        ],
      },
    ],
    socialLinks: [{ icon: "github", link: repo }],
    search: { provider: "local" },
    editLink: { pattern: `${repo}/edit/main/docs/:path`, text: "Edit this page on GitHub" },
    footer: { message: "MIT licensed. Provided as-is.", copyright: "Diana Nerd" },
  },
});
