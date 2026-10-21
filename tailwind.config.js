/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: {
    extend: {
      colors: {
        // Paleta institucional ANAM sobre tema oscuro
        ground:  "#0A1A17",
        bar:     "#071310",
        side:    "#0C201C",
        panel:   "#0F2622",
        term:    "#06110F",
        ink:     "#EEE8DD",
        ink2:    "#C9C2B6",
        muted:   "#A9B7B1",
        faint:   "#7E918A",
        gold:    "#BC955C",
        goldtxt: "#D9B783",
        goldhi:  "#EFD2A4",
        jade:    "#258466",
        jadetxt: "#7FCCAC",
        mint:    "#4FB58E",
        guinda:  "#9F2241",
        guindadk:"#691C32",
        rose:    "#EC8FA6",
        rosehi:  "#F2B3C3",
        warn:    "#E8C06A",
      },
      fontFamily: {
        sans: ['"Sora Variable"', '"Segoe UI"', "system-ui", "sans-serif"],
        mono: ['"JetBrains Mono"', '"Cascadia Mono"', "Consolas", "monospace"],
      },
      keyframes: {
        pulseDot: {
          "0%":   { boxShadow: "0 0 0 0 rgba(79,181,142,.55)" },
          "70%":  { boxShadow: "0 0 0 7px rgba(79,181,142,0)" },
          "100%": { boxShadow: "0 0 0 0 rgba(79,181,142,0)" },
        },
        shimmer: {
          from: { transform: "translateX(-100%)" },
          to:   { transform: "translateX(250%)" },
        },
        blink: { "50%": { opacity: "0" } },
      },
      animation: {
        "pulse-dot": "pulseDot 2s infinite",
        shimmer:     "shimmer 1.8s linear infinite",
        blink:       "blink 1s step-end infinite",
      },
    },
  },
  plugins: [],
};
