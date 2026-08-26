import { createRouter, createWebHashHistory } from 'vue-router';

import DevicesPage from '@/pages/devices-page.vue';
import InteractionLogsPage from '@/pages/interaction-logs-page.vue';
import OverviewPage from '@/pages/overview-page.vue';
import SipServicePage from '@/pages/sip-service-page.vue';

/** 桌面应用路由实例。 */
export const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    {
      path: '/',
      name: 'Overview',
      component: OverviewPage,
    },
    {
      path: '/devices',
      name: 'Devices',
      component: DevicesPage,
    },
    {
      path: '/interaction-logs',
      name: 'InteractionLogs',
      component: InteractionLogsPage,
    },
    {
      path: '/sip-service',
      name: 'SipService',
      component: SipServicePage,
    },
  ],
});
