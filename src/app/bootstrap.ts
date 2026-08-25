import { createPinia } from 'pinia';
import { createApp } from 'vue';

import App from '@/App.vue';
import { router } from '@/app/router';
import '@/styles/main.css';

/** 创建并挂载 GBLab 前端应用。 */
export function bootstrap(selector: string): void {
  const app = createApp(App);

  app.use(createPinia());
  app.use(router);
  app.mount(selector);
}
