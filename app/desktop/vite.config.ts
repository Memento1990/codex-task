import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
export default defineConfig(({command}) => ({ plugins: [svelte()], publicDir: command === 'build' ? false : 'public', clearScreen: false, build: { target: 'es2022' } }));
