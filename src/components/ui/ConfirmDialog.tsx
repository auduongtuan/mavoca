import React from 'react';
import { AlertDialog } from '@base-ui/react/alert-dialog';

interface ConfirmDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  description?: React.ReactNode;
  confirmLabel?: string;
  cancelLabel?: string;
  /** Styles the confirm button as a destructive action. */
  destructive?: boolean;
  onConfirm: () => void;
}

/**
 * A confirmation modal. Use it instead of `window.confirm`, which the macOS
 * webview does not implement. An alert dialog cannot be dismissed by clicking
 * outside it, so the user has to choose.
 */
export const ConfirmDialog: React.FC<ConfirmDialogProps> = ({
  open,
  onOpenChange,
  title,
  description,
  confirmLabel = 'Confirm',
  cancelLabel = 'Cancel',
  destructive = false,
  onConfirm,
}) => (
  <AlertDialog.Root open={open} onOpenChange={onOpenChange}>
    <AlertDialog.Portal>
      <AlertDialog.Backdrop className="confirm-backdrop" />
      <AlertDialog.Popup className="confirm-popup">
        <AlertDialog.Title className="confirm-title">{title}</AlertDialog.Title>
        {description && (
          <AlertDialog.Description className="confirm-description">
            {description}
          </AlertDialog.Description>
        )}
        <div className="confirm-actions">
          <AlertDialog.Close className="secondary-btn">{cancelLabel}</AlertDialog.Close>
          <button
            className={destructive ? 'danger-btn' : 'primary-btn'}
            onClick={() => {
              onOpenChange(false);
              onConfirm();
            }}
          >
            {confirmLabel}
          </button>
        </div>
      </AlertDialog.Popup>
    </AlertDialog.Portal>
  </AlertDialog.Root>
);
