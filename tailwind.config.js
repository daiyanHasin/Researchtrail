/** Colours come from CSS variables (RGB triplets) in web/src/input.css,
 *  so light and dark themes switch without rebuilding. */
const v = (name) => `rgb(var(--${name}) / <alpha-value>)`;
module.exports = {
  content: ["./web/index.html", "./web/app.js"],
  darkMode: ["class", '[data-theme="dark"]'],
  theme: {
    extend: {
      colors: {
        bg: v("bg"), surface: v("surface"), raised: v("raised"), sunken: v("sunken"),
        line: v("line"), ink: v("ink"), ink2: v("ink2"), ink3: v("ink3"),
        accent: v("accent"), accentink: v("accentink"), mine: v("mine"), warn: v("warn"), bad: v("bad"), good: v("good"),
      },
      fontFamily: {
        sans: ['"Segoe UI Variable Text"', '"Segoe UI"', '"SF Pro Text"', "-apple-system", "system-ui", "Roboto", "sans-serif"],
        serif: ['"Iowan Old Style"', '"Charter"', '"Bitstream Charter"', '"Source Serif Pro"', '"Sitka Text"', "Cambria", "Georgia", "serif"],
        mono: ['"Cascadia Code"', '"JetBrains Mono"', '"SF Mono"', "ui-monospace", "Consolas", "monospace"],
      },
      borderRadius: { card: "14px" },
      boxShadow: { soft: "0 1px 2px rgb(var(--shadow) / 0.06), 0 8px 24px -12px rgb(var(--shadow) / 0.18)" },
    },
  },
  plugins: [],
};
