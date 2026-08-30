<script setup lang="ts">
  import { computed } from 'vue';

  const props = withDefaults(
    defineProps<{
      icon: string;
      size?: number;
      strokeWidth?: number;
    }>(),
    { size: 16, strokeWidth: 1.8 },
  );

  const paths: Record<string, string[]> = {
    gauge: ['M4 13a8 8 0 1 1 16 0', 'M12 13l3-3', 'M6.5 7.5h.01', 'M17.5 7.5h.01'],
    server: ['M4 4h16v6H4z', 'M4 14h16v6H4z', 'M7 7h.01', 'M7 17h.01'],
    logs: ['M5 4h14v16H5z', 'M8 8h8', 'M8 12h8', 'M8 16h5'],
    video: ['M4 6h11v12H4z', 'M15 10l5-3v10l-5-3z'],
    settings: [
      'M12 15.5a3.5 3.5 0 1 0 0-7 3.5 3.5 0 0 0 0 7z',
      'M19.4 15a1.7 1.7 0 0 0 .03 1.8l.05.08-1.5 1.5-.08-.05a1.7 1.7 0 0 0-1.8-.03l-.1.04a1.7 1.7 0 0 0-1.03 1.56V20h-2v-.1a1.7 1.7 0 0 0-1.03-1.56l-.1-.04a1.7 1.7 0 0 0-1.8.03l-.08.05-1.5-1.5.05-.08A1.7 1.7 0 0 0 8.6 15l-.04-.1A1.7 1.7 0 0 0 7 13.87H6v-2h1a1.7 1.7 0 0 0 1.56-1.03l.04-.1a1.7 1.7 0 0 0-.03-1.8l-.05-.08 1.5-1.5.08.05a1.7 1.7 0 0 0 1.8.03l.1-.04A1.7 1.7 0 0 0 13.03 5V4h2v1a1.7 1.7 0 0 0 1.03 1.56l.1.04a1.7 1.7 0 0 0 1.8-.03l.08-.05 1.5 1.5-.05.08a1.7 1.7 0 0 0-.03 1.8l.04.1A1.7 1.7 0 0 0 21 11.87h1v2h-1a1.7 1.7 0 0 0-1.56 1.03z',
    ],
    search: ['M11 19a8 8 0 1 0 0-16 8 8 0 0 0 0 16z', 'm21 21-4.3-4.3'],
    filter: ['M4 5h16l-6 7v5l-4 2v-7z'],
    refresh: ['M20 11a8 8 0 0 0-14.9-3', 'M4 5v4h4', 'M4 13a8 8 0 0 0 14.9 3', 'M20 19v-4h-4'],
    save: ['M5 4h12l2 2v14H5z', 'M8 4v6h8V4', 'M8 20v-6h8v6'],
    reset: ['M4 12a8 8 0 1 0 2.3-5.6', 'M4 5v5h5'],
    copy: ['M8 8h11v12H8z', 'M5 16H4V4h12v1'],
    message: ['M5 5h14v11H8l-3 3z', 'M8 9h8', 'M8 12h5'],
    trash: ['M5 7h14', 'M10 11v6', 'M14 11v6', 'M7 7l1 13h8l1-13', 'M9 7V4h6v3'],
    edit: ['M4 20h4l11-11a2.1 2.1 0 0 0-3-3L5 17z', 'm14 7 3 3'],
    plus: ['M12 5v14', 'M5 12h14'],
    radio: ['M4 12a8 8 0 0 1 16 0', 'M7 12a5 5 0 0 1 10 0', 'M10 12a2 2 0 0 1 4 0', 'M12 12v8'],
    stop: ['M6 6h12v12H6z'],
    rows: ['M5 5h14', 'M5 12h14', 'M5 19h14'],
    check: ['m5 12 4 4L19 6'],
    x: ['M6 6l12 12', 'M18 6 6 18'],
    play: ['m8 5 11 7-11 7z'],
    bell: ['M18 8a6 6 0 0 0-12 0c0 7-3 7-3 9h18c0-2-3-2-3-9', 'M10 21h4'],
    alert: ['M12 3 2 20h20z', 'M12 9v4', 'M12 17h.01'],
    pin: [
      'M12 21s7-5.4 7-11a7 7 0 1 0-14 0c0 5.6 7 11 7 11z',
      'M12 13a2 2 0 1 0 0-4 2 2 0 0 0 0 4z',
    ],
    file: ['M6 3h8l4 4v14H6z', 'M14 3v5h5'],
    globe: [
      'M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18z',
      'M3 12h18',
      'M12 3a14 14 0 0 1 0 18',
      'M12 3a14 14 0 0 0 0 18',
    ],
    arrowDown: ['M12 4v16', 'm6 14 6 6 6-6'],
    arrowLeft: ['M19 12H5', 'm11 18-6-6 6-6'],
    arrowRight: ['M5 12h14', 'm13 6 6-6-6-6'],
  };

  const iconPaths = computed(() => paths[props.icon] ?? paths.file);
</script>

<template>
  <svg
    class="app-icon"
    :width="size"
    :height="size"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    :stroke-width="strokeWidth"
    stroke-linecap="round"
    stroke-linejoin="round"
    aria-hidden="true"
  >
    <path v-for="path in iconPaths" :key="path" :d="path" />
  </svg>
</template>
