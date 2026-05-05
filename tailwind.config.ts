import type { Config } from "tailwindcss";
import animate from "tailwindcss-animate";

export default {
  darkMode: ["class"],
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: {
    extend: {
      colors: {
        cream: "#FAF6EE",
        ink: "#2A2620",
        accent: "#C8553D",
        paper: "#FFFCF5",
        shelf: "#8B5A3C",
      },
      fontFamily: {
        sans: ["Inter", "Noto Sans SC", "system-ui", "sans-serif"],
        zh: ["Noto Sans SC", "Inter", "system-ui", "sans-serif"],
        handwritten: ["Caveat", "cursive"],
      },
      borderRadius: {
        lg: "12px",
        md: "8px",
        sm: "4px",
      },
    },
  },
  plugins: [animate],
} satisfies Config;
