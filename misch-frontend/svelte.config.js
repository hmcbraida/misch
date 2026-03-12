import adapter from '@sveltejs/adapter-static';

const basePath = process.env.BASE_PATH ?? '';

/** @type {import('@sveltejs/kit').Config} */
const config = {
	kit: {
		paths: {
			base: basePath
		},
		adapter: adapter({
			pages: 'build',
			assets: 'build',
			precompress: true
		})
	},
	vitePlugin: {
		dynamicCompileOptions: ({ filename }) => ({ runes: !filename.includes('node_modules') })
	}
};

export default config;
