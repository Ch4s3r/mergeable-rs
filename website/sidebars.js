// @ts-check

/** @type {import('@docusaurus/plugin-content-docs').SidebarsConfig} */
const sidebars = {
  documentation: [
    {
      type: 'category',
      label: 'Getting started',
      collapsed: false,
      items: ['intro', 'getting-started/usage', 'getting-started/support'],
    },
    {
      type: 'category',
      label: 'Configuration',
      link: {type: 'doc', id: 'configuration/configuration'},
      items: ['configuration/settings-file', 'configuration/events'],
    },
    {
      type: 'category',
      label: 'Validators',
      link: {
        type: 'generated-index',
        slug: '/validators',
        description: 'Validators check a pull request or issue and decide whether the pass, fail or error actions run.',
      },
      items: [
        'validators/age',
        'validators/approvals',
        'validators/assignee',
        'validators/author',
        'validators/base-ref',
        'validators/changeset',
        'validators/commit',
        'validators/contents',
        'validators/dependent',
        'validators/description',
        'validators/head-ref',
        'validators/label',
        'validators/last-comment',
        'validators/milestone',
        'validators/project',
        'validators/size',
        'validators/stale',
        'validators/title',
      ],
    },
    {
      type: 'category',
      label: 'Filters',
      link: {
        type: 'generated-index',
        slug: '/filters',
        description: 'Filters decide whether the validators of a recipe run at all.',
      },
      items: [
        'filters/author',
        'filters/base-ref',
        'filters/payload',
        'filters/repository',
      ],
    },
    {
      type: 'category',
      label: 'Actions',
      link: {
        type: 'generated-index',
        slug: '/actions',
        description: 'Actions run after the validation, depending on its outcome.',
      },
      items: [
        'actions/assign',
        'actions/checks',
        'actions/close',
        'actions/comment',
        'actions/labels',
        'actions/merge',
        'actions/request-review',
      ],
    },
    {
      type: 'category',
      label: 'Operators',
      link: {
        type: 'generated-index',
        slug: '/operators',
        description: 'Operators combine validators or filters into more complex logic.',
      },
      items: [
        'operators/and',
        'operators/or',
        'operators/not',
      ],
    },
    {
      type: 'category',
      label: 'Options',
      link: {
        type: 'generated-index',
        slug: '/options',
        description: 'Options are the building blocks of validators and filters.',
      },
      items: [
        'options/begins-with',
        'options/boolean',
        'options/ends-with',
        'options/jira',
        'options/max',
        'options/min',
        'options/must-exclude',
        'options/must-include',
        'options/no-empty',
        'options/one-of-none-of',
        'options/required',
      ],
    },
    {
      type: 'category',
      label: 'Annotations & templates',
      items: ['annotations/annotations', 'annotations/templates'],
    },
    'recipes',
    {
      type: 'category',
      label: 'Deployment',
      items: [
        'deployment/github-app',
        'deployment/container-image',
        'deployment/environment-variables',
        'deployment/running-locally',
      ],
    },
    {
      type: 'category',
      label: 'Operations',
      items: [
        'operations/overview',
        'operations/caching',
        'operations/scheduler',
        'operations/security',
      ],
    },
    {
      type: 'category',
      label: 'Contributing',
      items: [
        'contributing/contributing',
        'contributing/releases',
        'contributing/changelog',
        'contributing/licenses',
        'contributing/original-project-history',
      ],
    },
  ],
};

export default sidebars;
