import React, { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { AppSettings } from '../types';

const defaultDropboxPath = '~/Dropbox/Vocab Learning/vocab.sqlite3';

export const SettingsPanel: React.FC = () => {
  const [settings, setSettings] = useState<AppSettings>({ backupPath: '' });
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [launchAtLogin, setLaunchAtLogin] = useState(false);
  const [launchBusy, setLaunchBusy] = useState(false);

  useEffect(() => {
    invoke<AppSettings>('get_settings')
      .then(setSettings)
      .catch((err) => setError(err instanceof Error ? err.message : String(err)))
      .finally(() => setLoading(false));

    invoke<boolean>('get_launch_at_login')
      .then(setLaunchAtLogin)
      .catch((err) => setError(err instanceof Error ? err.message : String(err)));
  }, []);

  // Applies at once rather than on Save: the login item belongs to macOS, not
  // to the settings table the Save button writes.
  const toggleLaunchAtLogin = async (enabled: boolean) => {
    setLaunchBusy(true);
    setError(null);
    setMessage(null);
    try {
      const actual = await invoke<boolean>('set_launch_at_login', { enabled });
      setLaunchAtLogin(actual);
      setMessage(actual ? 'Vocab will open when you log in.' : 'Vocab will no longer open at login.');
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLaunchBusy(false);
    }
  };

  const save = async () => {
    setBusy(true);
    setError(null);
    setMessage(null);
    try {
      const nextSettings = await invoke<AppSettings>('update_settings', { settings });
      setSettings(nextSettings);
      setMessage(nextSettings.backupPath ? 'Backup location saved.' : 'Automatic backup disabled.');
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <main className="settings-shell">
      <section className="settings-panel">
        <div className="editor-header">
          <div>
            <p className="eyebrow">Startup and backup</p>
            <h1>Settings</h1>
          </div>
        </div>

        <label className="settings-toggle">
          <input
            type="checkbox"
            checked={launchAtLogin}
            onChange={(event) => toggleLaunchAtLogin(event.target.checked)}
            disabled={launchBusy}
          />
          <span>
            <strong>Open at login</strong>
            <small>Start the menu bar app automatically when you log in to your Mac.</small>
          </span>
        </label>

        <div className="settings-field">
          <label htmlFor="backupPath">Backup file path</label>
          <input
            id="backupPath"
            value={settings.backupPath}
            onChange={(event) => setSettings({ backupPath: event.target.value })}
            placeholder={defaultDropboxPath}
            disabled={loading || busy}
          />
          <p>
            Leave empty to disable backup. Folder paths are accepted and will use
            <code>vocab.sqlite3</code>.
          </p>
        </div>

        {error && <div className="manager-alert error">{error}</div>}
        {message && <div className="manager-alert success">{message}</div>}

        <div className="editor-actions">
          <button
            className="secondary-btn"
            onClick={() => setSettings({ backupPath: '' })}
            disabled={loading || busy}
          >
            Disable backup
          </button>
          <button
            className="secondary-btn"
            onClick={() => setSettings({ backupPath: defaultDropboxPath })}
            disabled={loading || busy}
          >
            Use Dropbox
          </button>
          <button
            className="primary-btn"
            onClick={save}
            disabled={loading || busy}
          >
            {busy ? 'Saving...' : 'Save settings'}
          </button>
        </div>
      </section>
    </main>
  );
};
