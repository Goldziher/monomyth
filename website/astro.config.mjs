// @ts-check
import { defineConfig } from "astro/config";
import starlight from "@astrojs/starlight";
import starlightLlmsTxt from "starlight-llms-txt";

// Project GitHub Pages: served under /monomyth/ at goldziher.github.io.
export default defineConfig({
  site: "https://goldziher.github.io",
  base: "/monomyth",
  integrations: [
    starlight({
      title: "monomyth",
      description:
        "An engine for narrative structure. A deterministic, corpus-grounded world/story model " +
        "rendered to text adventures across genres today, and pixel-art games next.",
      logo: {
        src: "./src/assets/mark.svg",
        alt: "monomyth",
      },
      favicon: "/favicon.svg",
      customCss: ["./src/styles/custom.css"],
      social: [{ icon: "github", label: "GitHub", href: "https://github.com/Goldziher/monomyth" }],
      editLink: {
        baseUrl: "https://github.com/Goldziher/monomyth/edit/main/website/",
      },
      head: [
        {
          tag: "link",
          attrs: { rel: "apple-touch-icon", href: "/monomyth/apple-touch-icon.png" },
        },
        {
          tag: "link",
          attrs: { rel: "icon", type: "image/png", sizes: "32x32", href: "/monomyth/favicon-32.png" },
        },
        {
          tag: "meta",
          attrs: { property: "og:image", content: "https://goldziher.github.io/monomyth/og.png" },
        },
        {
          tag: "meta",
          attrs: { name: "twitter:card", content: "summary_large_image" },
        },
        {
          tag: "meta",
          attrs: { name: "twitter:image", content: "https://goldziher.github.io/monomyth/og.png" },
        },
      ],
      plugins: [
        starlightLlmsTxt({
          promote: ["index*", "start/**", "concepts/**"],
          minify: { collapseCodeBlocks: true },
        }),
      ],
      sidebar: [
        {
          label: "Start here",
          items: [
            { label: "Introduction", slug: "start/introduction" },
            { label: "Installation", slug: "start/installation" },
            { label: "Quickstart", slug: "start/quickstart" },
          ],
        },
        {
          label: "Concepts",
          items: [
            { label: "Architecture", slug: "concepts/architecture" },
            { label: "Hybrid generation", slug: "concepts/hybrid-generation" },
            { label: "The narrative graph", slug: "concepts/narrative-graph" },
            { label: "Genres", slug: "concepts/genres" },
            { label: "Frontends", slug: "concepts/frontends" },
            { label: "Corpus & licensing", slug: "concepts/corpus-licensing" },
          ],
        },
        {
          label: "Reference",
          items: [
            { label: "Crates", slug: "reference/crates" },
            { label: "CLI", slug: "reference/cli" },
            { label: "Configuration", slug: "reference/configuration" },
            { label: "Frameworks", slug: "reference/frameworks" },
          ],
        },
        {
          label: "Decisions",
          items: [{ label: "Architecture decisions", slug: "decisions" }],
        },
      ],
    }),
  ],
});
