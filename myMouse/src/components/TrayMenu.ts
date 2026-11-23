export type TrayActions = {
  show: () => void;
  quit: () => void;
};

export function buildTrayMenuTemplate(actions: TrayActions) {
  return [
    { label: 'Show myMouse', click: actions.show },
    { type: 'separator' as const },
    { label: 'Quit', click: actions.quit },
  ];
}
