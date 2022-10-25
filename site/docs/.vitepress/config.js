export default {
    title: 'log-ship',
    description: "The world's most user-friendly log shipper",
    outDir: '../dist',
    themeConfig: {
        nav: [
            { text: 'Intro', link: '/intro' },
            {
                text: 'Documentation',
                items: [
                    {text: 'Running log-ship', link: '/running'},
                    {text: 'Configuration File', link: '/config'},
                    {text: 'Integrations', link: '/integrations'},
                ]
            },
            { text: 'Download', link: 'download' }
        ],
        outline: [2,4],
        sidebar: [
            {
                // text: 'Guide',
                items: [
                    { text: 'Introduction', link: '/intro' },
                    { text: 'Documentation', items: [
                            {text: 'Running log-ship', link: '/running'},
                            {text: 'Configuration File', link: '/config'},
                            {text: 'Integrations', link: '/integrations'},
                        ]
                    },
                    { text: 'Download', link: '/download' },
                ]
            }
        ],
        footer: {
            message: '<a href="http://github.com/log-store/log-ship">Source</a> | <a href="/license.html">License</a>',
            // copyright: 'Copyright © 2022-present'
        }
    }
}
