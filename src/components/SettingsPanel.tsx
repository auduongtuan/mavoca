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

  useEffect(() => {
    invoke<AppSettings>('get_settings')
      .then(setSettings)
      .catch((err) => setError(err instanceof Error ? err.message : String(err)))
      .finally(() => setLoading(false));
  }, []);

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
            <p className="eyebrow">Vocabulary backup</p>
            <h1>Settings</h1>
          </div>
        </div>

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
