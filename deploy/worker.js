// switcher.diananerd.com
//
//   curl -fsSL https://switcher.diananerd.com | sh     -> the installer
//   https://switcher.diananerd.com in a browser         -> the docs
//
// Command-line clients asking for / get the installer from the repository's
// main branch (its single source of truth); /install.sh always does. Every
// other request is the docs site, served from static assets.

const REPO = "https://github.com/diananerd/claude-account-switcher";
const INSTALLER = "https://raw.githubusercontent.com/diananerd/claude-account-switcher/main/install.sh";

// curl, wget and friends; an empty user agent is a script too.
const CLI = /^(curl|wget|libcurl|fetch|httpie|aria2|powershell|go-http-client|python-requests)\b/i;

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    const ua = request.headers.get("user-agent") || "";
    const wantsScript = url.pathname === "/install.sh" || (url.pathname === "/" && (ua === "" || CLI.test(ua)));
    if (!wantsScript) {
      return env.ASSETS.fetch(request);
    }
    const upstream = await fetch(INSTALLER, { cf: { cacheTtl: 300, cacheEverything: true } });
    if (!upstream.ok) {
      // A failing status makes `curl -f` stop, so `sh` runs nothing.
      return new Response(`installer unavailable (GitHub answered ${upstream.status}); see ${REPO}\n`, {
        status: 502,
        headers: { "content-type": "text/plain; charset=utf-8" },
      });
    }
    return new Response(upstream.body, {
      headers: {
        "content-type": "text/x-shellscript; charset=utf-8",
        "cache-control": "public, max-age=300",
        "x-content-source": INSTALLER,
      },
    });
  },
};
