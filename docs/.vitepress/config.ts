import { defineConfig } from 'vitepress'
import { withMermaid } from 'vitepress-plugin-mermaid'

export default withMermaid(
  defineConfig({
    vite: {
      optimizeDeps: {
        include: ['mermaid'],
      },
    },
    title: 'reincarnate',
    description: 'Legacy software lifting framework',

    base: '/reincarnate/',

    srcExclude: ['**/CLAUDE.md'],

    themeConfig: {
      nav: [
        { text: 'Guide', link: '/introduction' },
        { text: 'Targets', link: '/targets/' },
        { text: 'rhi', link: 'https://docs.rhi.zone/' },
      ],

      sidebar: {
        '/': [
          {
            text: 'Guide',
            items: [
              { text: 'Introduction', link: '/introduction' },
              { text: 'Getting Started', link: '/getting-started' },
              { text: 'Architecture', link: '/architecture' },
            ]
          },
          {
            text: 'Targets',
            items: [
              { text: 'Overview', link: '/targets/' },
              {
                text: 'Active',
                items: [
                  { text: 'Flash (AVM2)', link: '/targets/flash' },
                  { text: 'GameMaker (GML)', link: '/targets/gamemaker' },
                  {
                    text: 'Twine',
                    link: '/targets/twine',
                    items: [
                      { text: 'SugarCube', link: '/targets/sugarcube' },
                      { text: 'Harlowe', link: '/targets/harlowe' },
                    ]
                  },
                ]
              },
              {
                text: 'Planned',
                items: [
                  { text: "Director / Shockwave", link: '/targets/director' },
                  { text: "Ren'Py", link: '/targets/renpy' },
                  { text: 'RPG Maker', link: '/targets/rpgmaker' },
                  { text: 'Inform (Z-machine/Glulx)', link: '/targets/inform' },
                  { text: 'Ink by Inkle', link: '/targets/ink' },
                  { text: 'Visual Basic 6', link: '/targets/vb6' },
                  { text: 'Java Applets', link: '/targets/java-applets' },
                  { text: 'Silverlight', link: '/targets/silverlight' },
                  { text: 'HyperCard / ToolBook', link: '/targets/hypercard' },
                  { text: 'WolfRPG', link: '/targets/wolfrpg' },
                  { text: 'SRPG Studio', link: '/targets/srpg-studio' },
                  { text: 'RAGS', link: '/targets/rags' },
                  { text: 'QSP', link: '/targets/qsp' },
                  { text: 'PuzzleScript', link: '/targets/puzzlescript' },
                ]
              },
            ]
          },
          {
            text: 'Design',
            items: [
              { text: 'Philosophy', link: '/philosophy' },
              { text: 'Tier 1 vs Tier 2', link: '/tiers' },
              { text: 'Persistence & Saving', link: '/persistence' },
            ]
          },
        ]
      },

      socialLinks: [
        { icon: 'github', link: 'https://github.com/rhi-zone/reincarnate' }
      ],

      search: {
        provider: 'local'
      },

      editLink: {
        pattern: 'https://github.com/rhi-zone/reincarnate/edit/master/docs/:path',
        text: 'Edit this page on GitHub'
      },
    },
  }),
)
