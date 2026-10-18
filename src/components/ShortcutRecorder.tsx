import React, { useState } from 'react';

// Accelerator modifiers in the order Rust's parser and macOS menus both use.
const MODIFIERS = [
  { flag: 'ctrlKey', accelerator: 'Ctrl', symbol: '⌃' },
  { flag: 'altKey', accelerator: 'Alt', symbol: '⌥' },
  { flag: 'shiftKey', accelerator: 'Shift', symbol: '⇧' },
  { flag: 'metaKey', accelerator: 'Cmd', symbol: '⌘' },
] as const;

const MODIFIER_CODES = new Set([
  'ControlLeft', 'ControlRight', 'AltLeft', 'AltRight',
  'ShiftLeft', 'ShiftRight', 'MetaLeft', 'MetaRight', 'CapsLock', 'Fn',
]);

const KEY_SYMBOLS: Record<string, string> = {
  Space: 'Space',
  Enter: '↩',
  Backspace: '⌫',
  Tab: '⇥',
  ArrowUp: '↑',
  ArrowDown: '↓',
  ArrowLeft: '←',
  ArrowRight: '→',
};

/** Turns a `KeyboardEvent.code` into the key name the accelerator parser takes. */
const keyFromCode = (code: string) => {
  if (code.startsWith('Key')) return code.slice(3);
  if (code.startsWith('Digit')) return code.slice(5);
  return code;
};

const recordShortcut = (event: React.KeyboardEvent) => {
  if (MODIFIER_CODES.has(event.code)) {
    return null;
  }
  const modifiers = MODIFIERS.filter(({ flag }) => event[flag]);
  // Shift alone would steal ordinary typing, so a global shortcut needs more.
  if (!modifiers.some(({ accelerator }) => accelerator !== 'Shift')) {
    return null;
  }
  return [...modifiers.map(({ accelerator }) => accelerator), keyFromCode(event.code)].join('+');
};

/** Shows an accelerator like `Cmd+Ctrl+Shift+N` the macOS way: ⌃⇧⌘N. */
export const formatShortcut = (accelerator: string) => {
  const parts = accelerator.split('+');
  const key = parts.pop() ?? '';
  const symbols = MODIFIERS
    .filter(({ accelerator: name }) => parts.includes(name))
    .map(({ symbol }) => symbol)
    .join('');
  return symbols + (KEY_SYMBOLS[key] ?? key);
};

interface ShortcutRecorderProps {
  id: string;
  value: string;
  onChange: (value: string) => void;
  disabled?: boolean;
}

export const ShortcutRecorder: React.FC<ShortcutRecorderProps> = ({
  id,
  value,
  onChange,
  disabled,
}) => {
  const [recording, setRecording] = useState(false);

  const handleKeyDown = (event: React.KeyboardEvent<HTMLButtonElement>) => {
    if (!recording) {
      return;
    }
    event.preventDefault();
    if (event.key === 'Escape') {
      setRecording(false);
      return;
    }
    const shortcut = recordShortcut(event);
    if (shortcut) {
      onChange(shortcut);
      setRecording(false);
    }
  };

  let label = 'None';
  if (recording) {
    label = 'Press a shortcut…';
  } else if (value) {
    label = formatShortcut(value);
  }

  return (
    <button
      id={id}
      type="button"
      className={recording ? 'shortcut-recorder recording' : 'shortcut-recorder'}
      onClick={() => setRecording(true)}
      onKeyDown={handleKeyDown}
      onBlur={() => setRecording(false)}
      disabled={disabled}
    >
      {label}
    </button>
  );
};
