import { createRouter, createWebHashHistory } from 'vue-router';

import DevicesPage from '@/pages/devices-page.vue';
import OverviewPage from '@/pages/overview-page.vue';
import SipServicePage from '@/pages/sip-service-page.vue';
import SubscriptionsPage from '@/pages/subscriptions-page.vue';

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
      path: '/subscriptions',
      name: 'Subscriptions',
      component: SubscriptionsPage,
    },
    {
      path: '/sip-service',
      name: 'SipService',
      component: SipServicePage,
    },
  ],
});
