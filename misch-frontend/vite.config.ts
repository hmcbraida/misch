import tailwindcss from "@tailwindcss/vite";
import { sveltekit } from "@sveltejs/kit/vite";
import { defineConfig } from "vite";

export default defineConfig({
	plugins: [tailwindcss(), sveltekit()],
	server: {
		proxy: {
			"/misch/api": {
				target: "http://127.0.0.1:8000",
				changeOrigin: true,
				rewrite: (path) => path.replace(/^\/misch/, ""),
			},
			"/api": {
				target: "http://127.0.0.1:8000",
				changeOrigin: true,
			},
		},
	},
});
