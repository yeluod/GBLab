import { createRouter, createWebHashHistory } from 'vue-router';

/** 桌面应用路由实例。 */
export const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    {
      path: '/',
      name: 'Overview',
      component: () => import('@/pages/overview-page.vue'),
    },
    {
      path: '/devices',
      name: 'Devices',
      component: () => import('@/pages/devices-page.vue'),
    },
    {
      path: '/simulation',
      name: 'Simulation',
      component: () => import('@/pages/simulation-page.vue'),
    },
    {
      path: '/query-console',
      name: 'QueryConsole',
      component: () => import('@/pages/query-console-page.vue'),
    },
    {
      path: '/scenarios',
      name: 'Scenarios',
      component: () => import('@/pages/scenarios-page.vue'),
    },
    {
      path: '/runtime-observability',
      name: 'RuntimeObservability',
      component: () => import('@/pages/runtime-observability-page.vue'),
    },
    {
      path: '/interaction-logs',
      name: 'InteractionLogs',
      component: () => import('@/pages/interaction-logs-page.vue'),
    },
    {
      path: '/media',
      name: 'Media',
      component: () => import('@/pages/media-page.vue'),
    },
    {
      path: '/global-settings',
      name: 'GlobalSettings',
      component: () => import('@/pages/global-settings-page.vue'),
    },
    {
      path: '/sip-service',
      redirect: { name: 'GlobalSettings' },
    },
  ],
});
