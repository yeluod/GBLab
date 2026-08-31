<script setup lang="ts">
  import {
    NConfigProvider,
    NDialogProvider,
    NMessageProvider,
    type GlobalThemeOverrides,
  } from 'naive-ui';
  import { RouterView } from 'vue-router';

  import AppLayout from '@/layouts/app-layout.vue';

  const themeOverrides: GlobalThemeOverrides = {
    common: {
      primaryColor: '#087ea4',
      primaryColorHover: '#066985',
      primaryColorPressed: '#055a71',
      primaryColorSuppl: '#087ea4',
      infoColor: '#2563a8',
      successColor: '#16805c',
      warningColor: '#b7791f',
      errorColor: '#c2413b',
      bodyColor: '#f4f7fa',
      cardColor: '#ffffff',
      modalColor: '#ffffff',
      popoverColor: '#ffffff',
      borderColor: '#e2e8f0',
      textColorBase: '#172033',
      textColor1: '#172033',
      textColor2: '#475569',
      textColor3: '#64748b',
      closeIconColor: '#64748b',
      closeIconColorHover: '#172033',
    },
    Card: {
      borderColor: '#e2e8f0',
      boxShadow: '0 8px 24px rgb(15 23 42 / 6%)',
      borderRadius: '6px',
    },
    Button: {
      borderRadiusMedium: '6px',
      borderRadiusSmall: '5px',
    },
    Input: {
      borderRadius: '6px',
    },
    Select: {
      peers: {
        InternalSelection: {
          borderRadius: '6px',
        },
      },
    },
    DataTable: {
      borderColor: '#e2e8f0',
      thColor: '#f8fafc',
      tdColor: '#ffffff',
      thTextColor: '#475569',
      tdTextColor: '#172033',
    },
  };
</script>

<template>
  <NConfigProvider :theme-overrides="themeOverrides">
    <NDialogProvider>
      <NMessageProvider>
        <AppLayout>
          <RouterView />
        </AppLayout>
      </NMessageProvider>
    </NDialogProvider>
  </NConfigProvider>
</template>
