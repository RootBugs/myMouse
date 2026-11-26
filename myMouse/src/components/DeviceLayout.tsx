import React from 'react';
import type { DeviceLayoutData, LayoutNode } from '../types';

const NODE_WIDTH = 180;
const NODE_HEIGHT = 96;
const GAP = 60;
const MARGIN = 30;
const COLUMNS = 2;

type Point = { x: number; y: number };

function nodeKey(deviceId: string, monitorIndex: number): string {
  return `${deviceId}:${monitorIndex}`;
}

function midpoint(node: LayoutNode, point: Point): Point {
  return { x: point.x + NODE_WIDTH / 2, y: point.y + NODE_HEIGHT / 2 };
}

interface DeviceLayoutProps {
  layout?: DeviceLayoutData;
}

export default function DeviceLayout({ layout = { nodes: [], edges: [] } }: DeviceLayoutProps) {
  const positions = new Map<string, Point>();
  const devicePoints = new Map<string, Point>();
  layout.nodes.forEach((node, index) => {
    const column = index % COLUMNS;
    const row = Math.floor(index / COLUMNS);
    const point = {
      x: MARGIN + column * (NODE_WIDTH + GAP),
      y: MARGIN + row * (NODE_HEIGHT + GAP),
    };
    positions.set(nodeKey(node.deviceId, node.monitorIndex), point);
    if (!devicePoints.has(node.deviceId)) devicePoints.set(node.deviceId, point);
  });

  const rows = Math.max(1, Math.ceil(layout.nodes.length / COLUMNS));
  const viewWidth = MARGIN * 2 + Math.min(COLUMNS, Math.max(1, layout.nodes.length)) * NODE_WIDTH
    + Math.max(0, Math.min(COLUMNS, Math.max(1, layout.nodes.length)) - 1) * GAP;
  const viewHeight = MARGIN * 2 + rows * NODE_HEIGHT + Math.max(0, rows - 1) * GAP;

  return (
    <section>
      <h1>Device layout</h1>
      {layout.nodes.length === 0 ? (
        <p className="hint">No layout configured yet.</p>
      ) : (
        <>
          <svg
            viewBox={`0 0 ${viewWidth} ${viewHeight}`}
            className="layout-canvas"
            role="img"
            aria-label="Device adjacency layout"
          >
            {layout.edges.map((edge, index) => {
              const from = devicePoints.get(edge.from);
              const to = devicePoints.get(edge.to);
              if (!from || !to) return null;
              const start = { x: from.x + NODE_WIDTH / 2, y: from.y + NODE_HEIGHT / 2 };
              const end = { x: to.x + NODE_WIDTH / 2, y: to.y + NODE_HEIGHT / 2 };
              return (
                <line
                  key={`${edge.from}-${edge.to}-${edge.side}-${index}`}
                  x1={start.x}
                  y1={start.y}
                  x2={end.x}
                  y2={end.y}
                  className="layout-edge"
                  data-testid="layout-edge"
                />
              );
            })}
            {layout.nodes.map((node) => {
              const point = positions.get(nodeKey(node.deviceId, node.monitorIndex));
              if (!point) return null;
              const center = midpoint(node, point);
              return (
                <g
                  key={nodeKey(node.deviceId, node.monitorIndex)}
                  role="group"
                  aria-label={`${node.deviceId} monitor ${node.monitorIndex}`}
                >
                  <rect
                    x={point.x}
                    y={point.y}
                    width={NODE_WIDTH}
                    height={NODE_HEIGHT}
                    rx={8}
                    className="layout-node"
                  />
                  <text x={center.x} y={center.y - 6} textAnchor="middle" className="layout-label">
                    {`${node.deviceId} : ${node.monitorIndex}`}
                  </text>
                  <text x={center.x} y={center.y + 16} textAnchor="middle" className="layout-detail">
                    {`${node.width}×${node.height}`}
                  </text>
                </g>
              );
            })}
          </svg>
          <p className="hint">Cursor crosses only configured edges. No wrap by default.</p>
        </>
      )}
    </section>
  );
}
