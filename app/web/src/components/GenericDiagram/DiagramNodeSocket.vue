<template>
  <v-group>
    <v-circle
      :config="{
        id: socket.uniqueKey,
        x,
        y,
        width: socketSize,
        height: socketSize,
        stroke: colors.stroke,
        strokeWidth: isHovered ? 2 : 1,
        fill: fillColor,
      }"
      @mouseover="onMouseOver"
      @mouseout="onMouseOut"
    />
    <v-text
      ref="socketLabelRef"
      :config="{
        x: socket.def.nodeSide === 'left' ? 15 : -nodeWidth + 15,
        y: y - SOCKET_SIZE / 2,
        verticalAlign: 'middle',
        align: socket.def.nodeSide === 'left' ? 'left' : 'right',
        height: SOCKET_SIZE,
        width: nodeWidth - 30,
        text: socket.def.label,
        padding: 0,
        fill: colors.labelText,
        wrap: 'none',
        ellipsis: true,
        listening: false,
        fontFamily: DIAGRAM_FONT_FAMILY,
        fontSize: 11,
        opacity: state === 'draw_edge_disabled' ? 0.5 : 1,
      }"
    />
  </v-group>
</template>

<script lang="ts" setup>
import * as _ from "lodash-es";
import { KonvaEventObject } from "konva/lib/Node";
import { computed, PropType } from "vue";
import tinycolor from "tinycolor2";
import { useTheme } from "@si/vue-lib/design-system";
import { useStatusStore } from "@/store/status.store";
import {
  DiagramDrawEdgeState,
  DiagramEdgeData,
  DiagramSocketData,
} from "./diagram_types";

import { SOCKET_SIZE, DIAGRAM_FONT_FAMILY } from "./diagram_constants";

const { theme } = useTheme();

const props = defineProps({
  socket: {
    type: Object as PropType<DiagramSocketData>,
    required: true,
  },
  connectedEdges: {
    type: Array as PropType<DiagramEdgeData[]>,
    default: () => [],
  },
  drawEdgeState: {
    type: Object as PropType<DiagramDrawEdgeState>,
    required: true,
  },
  x: { type: Number, default: 0 },
  y: { type: Number, default: 0 },
  nodeWidth: { type: Number, required: true },
  isHovered: Boolean,
  isSelected: Boolean,
});

const emit = defineEmits(["hover:start", "hover:end"]);

// TODO: do not use the stores here - we should keep the diagram as only generic "dumb" components
const statusStore = useStatusStore();
const socketStatus = computed(() =>
  statusStore.getSocketStatus(props.socket.parent.def.id, props.socket.def.id),
);

const fillColor = computed(() => {
  if (socketStatus.value === "queued") return "#F00";
  if (socketStatus.value === "running") return "#0F0";
  if (socketStatus.value === "completed") return "#00F";
  return colors.value.fill;
});

const isConnected = computed(() => {
  const actualEdges = _.reject(
    props.connectedEdges,
    (e) => e.def?.changeStatus === "deleted",
  );
  return actualEdges.length >= 1;
});

const state = computed(() => {
  if (props.drawEdgeState.active) {
    if (props.drawEdgeState.fromSocketKey === props.socket.uniqueKey)
      return "draw_edge_from";
    if (props.drawEdgeState.toSocketKey === props.socket.uniqueKey)
      return "draw_edge_to";
    if (
      props.drawEdgeState.possibleTargetSocketKeys.includes(
        props.socket.uniqueKey,
      )
    )
      return "draw_edge_enabled";
    return "draw_edge_disabled";
  }
  return isConnected.value ? "connected" : "empty";
});

const socketSize = computed(() => {
  // change size / appearance
  if (
    ["draw_edge_from", "draw_edge_to", "draw_edge_enabled"].includes(
      state.value,
    )
  )
    return SOCKET_SIZE + 5;
  if (state.value === "draw_edge_disabled") return SOCKET_SIZE / 2;
  if (props.isHovered) return SOCKET_SIZE + 2;
  return SOCKET_SIZE;
});

const colors = computed(() => {
  const isFilled = ["draw_edge_from", "draw_edge_to", "connected"].includes(
    state.value,
  );
  const primaryColor = tinycolor(theme.value === "dark" ? "#FFF" : "#000");
  const noFillColor = theme.value === "dark" ? "#000" : "#FFF";
  return {
    stroke: primaryColor.toRgbString(),
    fill: isFilled ? primaryColor.toRgbString() : noFillColor,
    labelText: theme.value === "dark" ? "#FFF" : "#000",
  };
});

function onMouseOver(e: KonvaEventObject<MouseEvent>) {
  emit("hover:start");
  e.cancelBubble = true;
}
function onMouseOut(e: KonvaEventObject<MouseEvent>) {
  emit("hover:end");
  e.cancelBubble = true;
}
</script>
