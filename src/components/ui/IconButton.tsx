import React from 'react';

interface IconButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  /** Used as both the tooltip and the accessible name. */
  label: string;
  /** `small` is for controls sitting inside a line of text, such as the
   *  definition stepper; `medium` is the footer's navigation. */
  size?: 'small' | 'medium';
}

/**
 * A quiet icon button: no border, no fill, and a background only under the
 * pointer. The popup's navigation is a row of these, where a permanent fill on
 * each one would compete with the card for attention; the card's one solid
 * button is its primary action.
 *
 * The padding that gives the pointer something to hit is cancelled by an equal
 * negative margin, so the button costs the layout only the icon itself: the
 * glyph lines up with the text and the card edge beside it, and the hover fill
 * grows outwards into the gap rather than pushing anything aside. Rows of them
 * add that padding back to their own `gap`, or neighbours would overlap.
 */
export const IconButton: React.FC<IconButtonProps> = ({
  label,
  size = 'medium',
  className,
  children,
  ...rest
}) => (
  <button
    type="button"
    className={[`icon-btn`, `icon-btn-${size}`, className].filter(Boolean).join(' ')}
    title={label}
    aria-label={label}
    {...rest}
  >
    {children}
  </button>
);
