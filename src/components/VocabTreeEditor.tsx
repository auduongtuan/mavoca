import React from 'react';
import { VocabDefinition, VocabEntry, VocabSenseGroup } from '../types';

const partOfSpeechOptions = [
  'noun',
  'verb',
  'adjective',
  'adverb',
  'pronoun',
  'preposition',
  'conjunction',
  'interjection',
  'phrase',
  'idiom',
  'phrasal verb',
  'unknown',
];

const emptyDefinition: VocabDefinition = { id: 0, definition: '', examples: [] };
const emptyGroup: VocabSenseGroup = { guideWord: '', definitions: [emptyDefinition] };
const emptyEntry: VocabEntry = {
  partOfSpeech: '',
  pronUk: '',
  pronUs: '',
  senseGroups: [emptyGroup],
};

export const createEmptyEntries = (): VocabEntry[] => [
  { ...emptyEntry, senseGroups: [{ ...emptyGroup, definitions: [{ ...emptyDefinition }] }] },
];

const replaceAt = <T,>(items: T[], index: number, next: T): T[] =>
  items.map((item, itemIndex) => (itemIndex === index ? next : item));

const removeAt = <T,>(items: T[], index: number): T[] =>
  items.filter((_, itemIndex) => itemIndex !== index);

interface VocabTreeEditorProps {
  entries: VocabEntry[];
  onChange: (entries: VocabEntry[]) => void;
  disabled?: boolean;
}

/**
 * Edits the three-level tree in place: part-of-speech entries, guide-word sense
 * groups, and the definitions inside them. Examples are edited as one per line
 * rather than as individually managed rows.
 */
export const VocabTreeEditor: React.FC<VocabTreeEditorProps> = ({
  entries,
  onChange,
  disabled = false,
}) => {
  const updateEntry = (entryIndex: number, patch: Partial<VocabEntry>) => {
    onChange(replaceAt(entries, entryIndex, { ...entries[entryIndex], ...patch }));
  };

  const updateGroup = (
    entryIndex: number,
    groupIndex: number,
    patch: Partial<VocabSenseGroup>,
  ) => {
    const entry = entries[entryIndex];
    updateEntry(entryIndex, {
      senseGroups: replaceAt(entry.senseGroups, groupIndex, {
        ...entry.senseGroups[groupIndex],
        ...patch,
      }),
    });
  };

  const updateDefinition = (
    entryIndex: number,
    groupIndex: number,
    definitionIndex: number,
    patch: Partial<VocabDefinition>,
  ) => {
    const group = entries[entryIndex].senseGroups[groupIndex];
    updateGroup(entryIndex, groupIndex, {
      definitions: replaceAt(group.definitions, definitionIndex, {
        ...group.definitions[definitionIndex],
        ...patch,
      }),
    });
  };

  return (
    <div className="tree-editor">
      {entries.map((entry, entryIndex) => (
        <section className="tree-entry" key={entryIndex}>
          <header className="tree-entry-header">
            <label className="tree-field">
              Part of speech
              <select
                value={entry.partOfSpeech}
                disabled={disabled}
                onChange={(event) =>
                  updateEntry(entryIndex, { partOfSpeech: event.target.value })
                }
              >
                <option value="" disabled>
                  Select
                </option>
                {Array.from(
                  new Set([
                    ...partOfSpeechOptions,
                    ...(entry.partOfSpeech ? [entry.partOfSpeech] : []),
                  ]),
                ).map((option) => (
                  <option key={option} value={option}>
                    {option}
                  </option>
                ))}
              </select>
            </label>

            <label className="tree-field">
              UK
              <input
                value={entry.pronUk}
                disabled={disabled}
                placeholder="/ˈeksəmpl/"
                onChange={(event) => updateEntry(entryIndex, { pronUk: event.target.value })}
              />
            </label>

            <label className="tree-field">
              US
              <input
                value={entry.pronUs}
                disabled={disabled}
                placeholder="/ˈeksæmpl/"
                onChange={(event) => updateEntry(entryIndex, { pronUs: event.target.value })}
              />
            </label>

            <button
              className="tree-remove"
              disabled={disabled || entries.length === 1}
              onClick={() => onChange(removeAt(entries, entryIndex))}
              title="Remove this part of speech"
            >
              Remove
            </button>
          </header>

          {entry.senseGroups.map((group, groupIndex) => (
            <div className="tree-group" key={groupIndex}>
              <div className="tree-group-header">
                <label className="tree-field wide">
                  Guide word
                  <input
                    value={group.guideWord}
                    disabled={disabled}
                    placeholder="MOVE QUICKLY"
                    onChange={(event) =>
                      updateGroup(entryIndex, groupIndex, { guideWord: event.target.value })
                    }
                  />
                </label>

                <button
                  className="tree-remove"
                  disabled={disabled || entry.senseGroups.length === 1}
                  onClick={() =>
                    updateEntry(entryIndex, {
                      senseGroups: removeAt(entry.senseGroups, groupIndex),
                    })
                  }
                  title="Remove this sense group"
                >
                  Remove
                </button>
              </div>

              {group.definitions.map((definition, definitionIndex) => (
                <div className="tree-definition" key={definitionIndex}>
                  <div className="tree-definition-header">
                    <span>Definition {definitionIndex + 1}</span>
                    <button
                      className="tree-remove"
                      disabled={disabled || group.definitions.length === 1}
                      onClick={() =>
                        updateGroup(entryIndex, groupIndex, {
                          definitions: removeAt(group.definitions, definitionIndex),
                        })
                      }
                      title="Remove this definition"
                    >
                      Remove
                    </button>
                  </div>

                  <label className="tree-field wide">
                    Meaning
                    <textarea
                      value={definition.definition}
                      disabled={disabled}
                      rows={2}
                      onChange={(event) =>
                        updateDefinition(entryIndex, groupIndex, definitionIndex, {
                          definition: event.target.value,
                        })
                      }
                    />
                  </label>

                  <label className="tree-field wide">
                    Examples (one per line)
                    <textarea
                      value={definition.examples.join('\n')}
                      disabled={disabled}
                      rows={2}
                      onChange={(event) =>
                        updateDefinition(entryIndex, groupIndex, definitionIndex, {
                          examples: event.target.value.split('\n'),
                        })
                      }
                    />
                  </label>
                </div>
              ))}

              <button
                className="tree-add"
                disabled={disabled}
                onClick={() =>
                  updateGroup(entryIndex, groupIndex, {
                    definitions: [...group.definitions, { ...emptyDefinition }],
                  })
                }
              >
                Add definition
              </button>
            </div>
          ))}

          <button
            className="tree-add"
            disabled={disabled}
            onClick={() =>
              updateEntry(entryIndex, {
                senseGroups: [
                  ...entry.senseGroups,
                  { guideWord: '', definitions: [{ ...emptyDefinition }] },
                ],
              })
            }
          >
            Add sense group
          </button>
        </section>
      ))}

      <button
        className="tree-add"
        disabled={disabled}
        onClick={() =>
          onChange([
            ...entries,
            {
              partOfSpeech: '',
              pronUk: '',
              pronUs: '',
              senseGroups: [{ guideWord: '', definitions: [{ ...emptyDefinition }] }],
            },
          ])
        }
      >
        Add part of speech
      </button>
    </div>
  );
};
