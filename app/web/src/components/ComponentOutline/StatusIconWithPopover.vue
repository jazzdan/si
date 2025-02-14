<template>
  <div
    :class="
      clsx(
        'p-2xs rounded',
        isOpen
          ? 'bg-action-300 dark:bg-action-400'
          : 'hover:bg-action-200 dark:hover:bg-action-300',
      )
    "
    @mouseenter="setHover(true)"
    @mouseleave="setHover(false)"
    @click="openPopover"
  >
    <StatusIndicatorIcon
      :type="type"
      :status="status"
      :size="size"
      :tone="isHovered || isOpen ? 'shade' : undefined"
    />
    <Teleport v-if="isOpen" to="body">
      <div
        v-if="popoverPosition"
        ref="popover"
        :style="{
          top: `${popoverPosition.y}px`,
          left: `${popoverPosition.x}px`,
        }"
        class="absolute z-50 ml-sm"
      >
        <slot />
      </div>
    </Teleport>
  </div>
</template>

<script setup lang="ts">
import { ref, PropType, onBeforeUnmount } from "vue";
import { IconSizes } from "@si/vue-lib/design-system";
import * as _ from "lodash-es";
import clsx from "clsx";
import StatusIndicatorIcon, {
  IconType,
} from "@/components/StatusIndicatorIcon2.vue";

const props = defineProps({
  type: { type: String as PropType<IconType>, required: true },
  status: { type: String },
  size: { type: String as PropType<IconSizes> },
  popoverPosition: { type: Object as PropType<{ x: number; y: number }> },
});

const popover = ref<HTMLElement>();

function onWindowMousedown(e: MouseEvent) {
  requestAnimationFrame(() => {
    if (e.target instanceof Element && popover.value?.contains(e.target)) {
      return; // Don't close on click inside popover
    }

    closePopover();
  });
}

function onKeyboardEvent(e: KeyboardEvent) {
  if (e.key === "Escape") {
    closePopover();
  }
}

function removeListeners() {
  window.removeEventListener("click", onWindowMousedown);
  window.removeEventListener("keydown", onKeyboardEvent);
}

onBeforeUnmount(() => {
  removeListeners();
});

const isHovered = ref(false);

function setHover(v: boolean) {
  isHovered.value = v;
}

const isOpen = ref(false);

function openPopover() {
  isOpen.value = true;
  window.addEventListener("mousedown", onWindowMousedown);
  window.addEventListener("keydown", onKeyboardEvent);
}

function closePopover() {
  isOpen.value = false;
  removeListeners();
}
</script>
