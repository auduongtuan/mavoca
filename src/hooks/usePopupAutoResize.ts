import { RefObject, useLayoutEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';

/**
 * Keeps the popup window as tall as the card. The card has its natural height
 * (it does not fill the window), so measuring it cannot feed back into itself.
 * Rust clamps the result to the screen; past that, the popup's scroll area
 * scrolls.
 */
export const usePopupAutoResize = (cardRef: RefObject<HTMLElement | null>) => {
  useLayoutEffect(() => {
    const card = cardRef.current;
    const container = card?.parentElement;
    if (!card || !container) {
      return;
    }

    let lastHeight = 0;
    const report = () => {
      const style = getComputedStyle(container);
      const padding = parseFloat(style.paddingTop) + parseFloat(style.paddingBottom);
      const height = Math.ceil(card.offsetHeight + padding);
      if (height === lastHeight) {
        return;
      }
      lastHeight = height;
      invoke('resize_popup', { height }).catch((err) => {
        console.error('Failed to resize popup', err);
      });
    };

    report();
    const observer = new ResizeObserver(report);
    observer.observe(card);
    return () => observer.disconnect();
  }, [cardRef]);
};
