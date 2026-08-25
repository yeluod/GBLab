<script setup lang="ts">
  import { darkTheme, NConfigProvider, NDialogProvider, NMessageProvider } from 'naive-ui';
  import { RouterView } from 'vue-router';

  import AppLayout from '@/layouts/app-layout.vue';
</script>

<template>
  <NConfigProvider :theme="darkTheme">
    <NDialogProvider>
      <NMessageProvider>
        <AppLayout>
          <RouterView />
        </AppLayout>
      </NMessageProvider>
    </NDialogProvider>
  </NConfigProvider>
</template>
