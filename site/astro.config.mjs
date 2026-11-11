// @ts-check
// Configuration of the Sigilfall documentation site: Starlight with English
// at the root and French under /fr/, served from GitHub Pages under
// /sigilfall. Internal links are checked at build time by
// starlight-links-validator; the rustdoc copied under /api/ by the docs
// workflow is not a Starlight page, so links to it are left out of the check.
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';
import starlightLinksValidator from 'starlight-links-validator';

export default defineConfig({
	site: 'https://enzotekrennes.github.io',
	base: '/sigilfall',
	integrations: [
		starlight({
			title: { en: 'Sigilfall', fr: 'Sigilfall' },
			description: 'A terminal roguelike where spells are cast by tracing sigils on the keyboard.',
			favicon: '/favicon.svg',
			defaultLocale: 'root',
			locales: {
				root: { label: 'English', lang: 'en' },
				fr: { label: 'Français', lang: 'fr' },
			},
			social: [
				{ icon: 'github', label: 'GitHub', href: 'https://github.com/Enzotekrennes/sigilfall' },
			],
			editLink: {
				baseUrl: 'https://github.com/Enzotekrennes/sigilfall/edit/main/site/',
			},
			customCss: ['./src/styles/theme.css'],
			plugins: [
				starlightLinksValidator({
					exclude: ['/sigilfall/api/**'],
				}),
			],
			sidebar: [
				{
					label: 'Play',
					translations: { fr: 'Jouer' },
					items: [
						{ slug: 'install' },
						{ slug: 'play' },
						{ slug: 'keyboard' },
						{ slug: 'language' },
						{ slug: 'files' },
					],
				},
				{
					label: 'Develop',
					translations: { fr: 'Développer' },
					items: [
						{ slug: 'architecture' },
						{ slug: 'contributing' },
					],
				},
				{
					label: 'Reference',
					translations: { fr: 'Référence' },
					items: [
						{ slug: 'reference/core' },
						{
							label: 'Rustdoc of sigilfall-core',
							translations: { fr: 'Rustdoc de sigilfall-core' },
							link: '/api/sigilfall_core/',
							attrs: { target: '_self' },
						},
					],
				},
			],
		}),
	],
});
