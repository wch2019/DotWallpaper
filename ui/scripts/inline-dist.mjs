import assert from "node:assert/strict";
import { readFile, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const dist = resolve(process.env.DW_DIST_DIR ?? resolve(here, "../dist"));
let html = await readFile(resolve(dist, "index.html"), "utf8");

const css = html.match(/<link[^>]+href=["']([^"']+\.css)["'][^>]*>/i);
if (!css) throw new Error("Built HTML is missing the stylesheet");
const cssPath = resolve(dist, css[1]);
const cssText = await readFile(cssPath, "utf8");
html = html.replace(css[0], () => `<style data-vite-inline>${cssText}</style>`);

const js = html.match(/<script[^>]+src=["']([^"']+\.js)["'][^>]*><\/script>/i);
if (!js) throw new Error("Built HTML is missing the bundled JavaScript");
const jsPath = resolve(dist, js[1]);
let jsText = await readFile(jsPath, "utf8");
// Prevent a literal </script> in compiled template strings from ending the inline element.
jsText = jsText.replaceAll("</script", "<\\/script");
// Vite puts its module script in <head>. A classic inline script there runs
// before #app exists; an inline module can be blocked by WKWebView file://
// restrictions. Move the self-contained bundle after the app root instead.
html = html.replace(js[0], "");
const bodyEnd = html.lastIndexOf("</body>");
if (bodyEnd < 0) throw new Error("Built HTML is missing </body>");
const rootEnd = html.indexOf('<div id="app"></div>');
if (rootEnd < 0 || rootEnd >= bodyEnd) throw new Error("Built HTML is missing #app before </body>");
html = `${html.slice(0, bodyEnd)}<script data-vite-inline>${jsText}</script>\n${html.slice(bodyEnd)}`;

// Fail the native build rather than shipping a silently blank WKWebView.
assert(html.indexOf('<div id="app"></div>') < html.indexOf('<script data-vite-inline>'));
assert(html.indexOf('<script data-vite-inline>') < html.lastIndexOf('</body>'));
assert(!/<script[^>]+src=|<link[^>]+href=["'][^"']+\.css/i.test(html));
await writeFile(resolve(dist, "index.html"), html);
console.log("Inlined Vite assets into dist/index.html for WKWebView file:// loading");
