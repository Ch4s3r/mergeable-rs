// @ts-check

import {themes as prismThemes} from 'prism-react-renderer';

const repositoryUrl = 'https://github.com/Ch4s3r/mergeable-rs';

/** @type {import('@docusaurus/types').Config} */
const config = {
  title: 'mergeable-rs',
  tagline: 'Automate your team\'s GitHub workflow without a single line of code',
  favicon: 'img/favicon.svg',

  future: {
    v4: true,
  },

  url: 'https://ch4s3r.github.io',
  baseUrl: '/mergeable-rs/',
  organizationName: 'Ch4s3r',
  projectName: 'mergeable-rs',
  trailingSlash: false,

  onBrokenLinks: 'throw',
  onBrokenAnchors: 'throw',
  markdown: {
    hooks: {
      onBrokenMarkdownLinks: 'throw',
    },
  },

  i18n: {
    defaultLocale: 'en',
    locales: ['en'],
  },

  presets: [
    [
      'classic',
      /** @type {import('@docusaurus/preset-classic').Options} */
      ({
        docs: {
          routeBasePath: '/',
          sidebarPath: './sidebars.js',
          editUrl: `${repositoryUrl}/edit/main/website/`,
        },
        blog: false,
        theme: {
          customCss: './src/css/custom.css',
        },
      }),
    ],
  ],

  themeConfig:
    /** @type {import('@docusaurus/preset-classic').ThemeConfig} */
    ({
      navbar: {
        title: 'mergeable-rs',
        logo: {
          alt: 'mergeable-rs logo',
          src: 'img/logo.svg',
        },
        items: [
          {
            type: 'docSidebar',
            sidebarId: 'documentation',
            position: 'left',
            label: 'Documentation',
          },
          {
            to: '/recipes',
            label: 'Recipes',
            position: 'left',
          },
          {
            href: `${repositoryUrl}/releases`,
            label: 'Releases',
            position: 'right',
          },
          {
            href: repositoryUrl,
            label: 'GitHub',
            position: 'right',
          },
        ],
      },
      footer: {
        style: 'dark',
        links: [
          {
            title: 'Documentation',
            items: [
              {label: 'Getting started', to: '/'},
              {label: 'Configuration', to: '/configuration'},
              {label: 'Deployment', to: '/deployment/github-app'},
            ],
          },
          {
            title: 'Project',
            items: [
              {label: 'GitHub', href: repositoryUrl},
              {label: 'Releases', href: `${repositoryUrl}/releases`},
              {label: 'Issues', href: `${repositoryUrl}/issues`},
            ],
          },
          {
            title: 'Original project',
            items: [
              {label: 'mergeability/mergeable', href: 'https://github.com/mergeability/mergeable'},
            ],
          },
        ],
        copyright: `mergeable-rs is licensed under AGPL-3.0, like the original mergeable project. Built with Docusaurus.`,
      },
      prism: {
        theme: prismThemes.github,
        darkTheme: prismThemes.dracula,
        additionalLanguages: ['bash'],
      },
    }),
};

export default config;
