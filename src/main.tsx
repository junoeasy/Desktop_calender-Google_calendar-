import React, { useEffect, useMemo, useRef, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import './style.css';

type Event = { id: string; calendar_id: string; title: string; start: string; end: string; all_day: boolean; recurring: boolean; link: string; color: string; writable: boolean };
type Task = { id: string; list_id: string; title: string; notes: string; due: string | null; completed: boolean };
type TaskList = { id: string; title: string };
type Calendar = { id: string; title: string; color: string };
type Snapshot = { events: Event[]; tasks: Task[]; task_lists: TaskList[]; calendars: Calendar[]; cached_at: string; offline: boolean };
type Settings = { client_id: string; hidden_calendar_ids: string[]; theme: 'midnight' | 'ocean' | 'forest'; opacity: number; desktop_mode: boolean; autostart: boolean; x: number | null; y: number | null; width: number; height: number };
type EventDraft = { id?: string; calendar_id?: string; title: string; date: string; start_time: string; end_time: string; all_day: boolean };
type TaskDraft = { id?: string; list_id: string; title: string; notes: string; due: string };
const empty: Snapshot = { events: [], tasks: [], task_lists: [], calendars: [], cached_at: '', offline: false };
const today = () => localDate(new Date());
function localDate(d: Date) { return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`; }
function dateOf(s: string) { return s.slice(0, 10); }
function monthOf(s: string) { return s.slice(0, 7); }
function shiftMonth(month: string, n: number) { const [y, m] = month.split('-').map(Number); const d = new Date(y, m - 1 + n, 1); return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}`; }
function monthCells(month: string) { const [y, m] = month.split('-').map(Number); const first = new Date(y, m - 1, 1); const offset = first.getDay(); const count = new Date(y, m, 0).getDate(); return Array.from({ length: Math.ceil((offset + count) / 7) * 7 }, (_, i) => { const d = new Date(y, m - 1, i - offset + 1); return localDate(d); }); }
function eventDays(e: Event) { const end = new Date(e.all_day ? `${e.end}T00:00:00` : e.end); if (e.all_day) end.setDate(end.getDate() - 1); else end.setMilliseconds(end.getMilliseconds() - 1); const start = new Date(e.all_day ? `${e.start}T00:00:00` : e.start); const out: string[] = []; for (const d = new Date(start.getFullYear(), start.getMonth(), start.getDate()); d <= end && out.length < 45; d.setDate(d.getDate() + 1)) out.push(localDate(d)); return out; }
function eventTime(e: Event) { return e.all_day ? '종일' : new Date(e.start).toLocaleTimeString('ko-KR', { hour: '2-digit', minute: '2-digit', hour12: false }); }
function errorText(e: unknown) { return e instanceof Error ? e.message : String(e); }

function App() {
  const [selected, setSelected] = useState(today());
  const [month, setMonth] = useState(monthOf(today()));
  const [data, setData] = useState<Snapshot>(empty);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [signedIn, setSignedIn] = useState(false);
  const [view, setView] = useState<'calendar' | 'settings'>('calendar');
  const [editor, setEditor] = useState<{ kind: 'event'; value: EventDraft } | { kind: 'task'; value: TaskDraft } | null>(null);
  const [busy, setBusy] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [message, setMessage] = useState('');
  const requestId = useRef(0);

  const refresh = async (target = month, force = true) => {
    const id = ++requestId.current;
    setRefreshing(true);
    try { const snapshot = await invoke<Snapshot>('sync_month', { month: target, force }); if (id !== requestId.current) return; setData(snapshot); if (snapshot.offline) setMessage('오프라인 · 마지막 저장 데이터를 표시합니다'); else setMessage(''); }
    catch (e) { if (id === requestId.current) setMessage(errorText(e)); }
    finally { if (id === requestId.current) setRefreshing(false); }
  };
  useEffect(() => {
    invoke<Settings>('get_settings').then(setSettings).catch(e => setMessage(errorText(e)));
    invoke<boolean>('auth_status').then(setSignedIn).catch(() => {});
    const mode = listen<boolean>('desktop-mode-changed', e => setSettings(s => s ? { ...s, desktop_mode: e.payload } : s));
    return () => { mode.then(f => f()); };
  }, []);
  useEffect(() => {
    const unlisten = [listen('sync-requested', () => refresh(month, true)), listen('sync-complete', () => refresh(month, false))];
    return () => { unlisten.forEach(p => p.then(f => f())); };
  }, [month]);
  useEffect(() => { if (monthOf(selected) !== month) setSelected(`${month}-01`); refresh(month, true); }, [month]);
  useEffect(() => { if (settings) document.documentElement.style.setProperty('--opacity', String(settings.opacity / 100)); }, [settings?.opacity]);
  useEffect(() => {
    const win = getCurrentWindow();
    let timer: number;
    const save = () => { clearTimeout(timer); timer = window.setTimeout(async () => { try { const p = await win.outerPosition(); const s = await win.outerSize(); await invoke('save_window_geometry', { x: p.x, y: p.y, width: s.width, height: s.height }); } catch {} }, 350); };
    const unsub = [win.onMoved(save), win.onResized(save)];
    return () => { clearTimeout(timer); unsub.forEach(p => p.then(f => f())); };
  }, []);

  const eventMap = useMemo(() => { const map = new Map<string, Event[]>(); for (const e of data.events) if (!settings?.hidden_calendar_ids.includes(e.calendar_id)) for (const day of eventDays(e)) map.set(day, [...(map.get(day) || []), e]); return map; }, [data.events, settings?.hidden_calendar_ids]);
  const taskMap = useMemo(() => { const map = new Map<string, Task[]>(); for (const task of data.tasks) if (!task.completed && task.due) { const day = dateOf(task.due); map.set(day, [...(map.get(day) || []), task]); } return map; }, [data.tasks]);
  const dayEvents = (eventMap.get(selected) || []).sort((a, b) => a.start.localeCompare(b.start));
  const dayTasks = taskMap.get(selected) || [];
  const calendars = data.calendars || [];
  const defaultList = data.task_lists[0]?.id || '';
  const mutate = async (command: string, args: Record<string, unknown>) => { setBusy(true); setMessage(''); try { await invoke(command, args); setEditor(null); await refresh(month, true); } catch (e) { setMessage(errorText(e)); } finally { setBusy(false); } };
  const saveSettings = async (next: Settings) => { setSettings(next); try { await invoke('save_settings', { settings: next }); } catch (e) { setMessage(errorText(e)); } };
  const toggleCalendar = (id: string, checked: boolean) => { if (!settings) return; const hidden = settings.hidden_calendar_ids.filter(x => x !== id); if (!checked) hidden.push(id); saveSettings({ ...settings, hidden_calendar_ids: hidden }); };
  const login = async () => { if (!settings?.client_id) { setView('settings'); setMessage('Google OAuth 클라이언트 ID를 먼저 입력하세요.'); return; } setBusy(true); try { await invoke('sign_in', { clientId: settings.client_id }); setSignedIn(true); await refresh(month, true); setView('calendar'); } catch (e) { setMessage(errorText(e)); } finally { setBusy(false); } };
  const logout = async () => { try { await invoke('sign_out'); setSignedIn(false); setData(empty); setMessage('연결을 해제했습니다'); } catch (e) { setMessage(errorText(e)); } };
  const editEvent = (e: Event) => { if (e.recurring || !e.writable) { invoke('open_link', { url: e.link }).catch(err => setMessage(errorText(err))); return; } const start = new Date(e.start), end = new Date(e.end); setEditor({ kind: 'event', value: { id: e.id, calendar_id: e.calendar_id, title: e.title, date: e.all_day ? dateOf(e.start) : localDate(start), start_time: `${String(start.getHours()).padStart(2, '0')}:${String(start.getMinutes()).padStart(2, '0')}`, end_time: `${String(end.getHours()).padStart(2, '0')}:${String(end.getMinutes()).padStart(2, '0')}`, all_day: e.all_day } }); };
  const saveEditor = () => { if (!editor) return; if (editor.kind === 'event') { const e = editor.value; if (!e.title.trim()) { setMessage('일정 제목을 입력하세요'); return; } const start = e.all_day ? e.date : new Date(`${e.date}T${e.start_time}`).toISOString(); const endDate = new Date(`${e.date}T00:00:00`); endDate.setDate(endDate.getDate() + 1); const end = e.all_day ? localDate(endDate) : new Date(`${e.date}T${e.end_time}`).toISOString(); if (!e.all_day && new Date(end) <= new Date(start)) { setMessage('종료 시간을 시작 시간보다 늦게 설정하세요'); return; } mutate('save_event', { input: { id: e.id || null, calendar_id: e.calendar_id || 'primary', title: e.title.trim(), start, end, all_day: e.all_day } }); } else { const t = editor.value; if (!t.title.trim()) { setMessage('할 일 제목을 입력하세요'); return; } mutate('save_task', { input: { id: t.id || null, list_id: t.list_id, title: t.title.trim(), notes: t.notes, due: t.due || null } }); } };
  const monthLabel = new Date(Number(month.slice(0, 4)), Number(month.slice(5)) - 1, 1).toLocaleDateString('ko-KR', { year: 'numeric', month: 'long' });
  return <div className="shell" data-theme={settings?.theme || 'midnight'}>
    <header className="topbar" data-tauri-drag-region>
      <div className="brand" data-tauri-drag-region><div className="brand-icon">◫</div><span>DAYFRAME</span><small>DESKTOP CALENDAR</small></div>
      <div className="top-actions"><span className={`status ${data.offline ? 'offline' : ''}`}>{signedIn ? data.offline ? '오프라인' : '연결됨' : '연결 전'}</span><button aria-label="설정" className={view === 'settings' ? 'active' : ''} onClick={() => setView(view === 'settings' ? 'calendar' : 'settings')}>⚙</button><button aria-label="창 숨기기" onClick={() => getCurrentWindow().hide()}>✕</button></div>
    </header>
    {message && <div className="notice" role="status"><span>{message}</span><button onClick={() => setMessage('')}>✕</button></div>}
    {view === 'settings' ? <main className="settings"><div className="eyebrow">PREFERENCES</div><h1>설정</h1><p className="muted">내 바탕화면에 맞게 달력을 조정하세요.</p>
      <section><h2>Google 계정</h2><label>Desktop OAuth 클라이언트 ID<input value={settings?.client_id || ''} disabled={signedIn} placeholder="...apps.googleusercontent.com" onChange={e => settings && setSettings({ ...settings, client_id: e.target.value })} onBlur={() => settings && saveSettings(settings)}/></label><div className="setting-row"><p>Google Calendar와 Tasks를 연결합니다.<br/><small>Cloud Console에서 Desktop app 클라이언트를 생성하세요.</small></p>{signedIn ? <div className="button-group"><button className="secondary" disabled={busy} onClick={login}>다시 로그인</button><button className="secondary" onClick={logout}>연결 해제</button></div> : <button className="primary" disabled={busy} onClick={login}>Google 로그인</button>}</div></section>
      <section><h2>화면</h2><label>테마<select value={settings?.theme || 'midnight'} onChange={e => settings && saveSettings({ ...settings, theme: e.target.value as Settings['theme'] })}><option value="midnight">미드나이트</option><option value="ocean">오션</option><option value="forest">포레스트</option></select></label><label>불투명도 <strong>{settings?.opacity || 85}%</strong><input type="range" min="35" max="100" value={settings?.opacity || 85} onChange={e => settings && setSettings({ ...settings, opacity: Number(e.target.value) })} onMouseUp={() => settings && saveSettings(settings)} onTouchEnd={() => settings && saveSettings(settings)}/></label><div className="setting-row"><p>바탕화면 위젯 모드<br/><small>Wayland에서는 일반 위젯 창으로 실행됩니다.</small></p><input type="checkbox" checked={settings?.desktop_mode || false} onChange={e => settings && saveSettings({ ...settings, desktop_mode: e.target.checked })}/></div><div className="setting-row"><p>시작할 때 자동 실행</p><input type="checkbox" checked={settings?.autostart || false} onChange={e => settings && saveSettings({ ...settings, autostart: e.target.checked })}/></div></section>
      <button className="text-button" onClick={() => setView('calendar')}>← 달력으로 돌아가기</button>
    </main> : <main className="layout"><section className="calendar-panel"><div className="calendar-head"><div><div className="eyebrow">YOUR MONTH AT A GLANCE</div><h1>{monthLabel}</h1></div><div className="month-nav"><button onClick={() => setMonth(shiftMonth(month, -1))}>‹</button><button className="today-button" onClick={() => { setSelected(today()); setMonth(monthOf(today())); }}>오늘</button><button onClick={() => setMonth(shiftMonth(month, 1))}>›</button></div></div><div className="weekdays">{['일', '월', '화', '수', '목', '금', '토'].map(x => <div key={x}>{x}</div>)}</div><div className="grid">{monthCells(month).map(day => { const events = eventMap.get(day) || [], tasks = taskMap.get(day) || []; return <button key={day} className={`cell ${monthOf(day) !== month ? 'outside' : ''} ${day === today() ? 'today' : ''} ${day === selected ? 'selected' : ''}`} onClick={() => setSelected(day)}><span className="day-number">{Number(day.slice(8))}</span><div className="cell-items">{events.slice(0, 2).map(e => <span className="event-chip" key={e.calendar_id + e.id} style={{ '--event-color': e.color || '#8c9cff' } as React.CSSProperties}>{e.title}</span>)}{tasks.length > 0 && <span className="task-chip">● {tasks.length} 할 일</span>}{events.length > 2 && <span className="more">+{events.length - 2} 일정</span>}</div></button>; })}</div><div className="calendar-foot"><span><i className="legend-dot"/> 일정</span><span><i className="legend-dot task"/> 할 일</span><button className="refresh-button" onClick={() => refresh(month, true)} disabled={refreshing || busy} aria-busy={refreshing}><span aria-hidden="true" className={refreshing ? 'refresh-icon spinning' : 'refresh-icon'}>↻</span> {refreshing ? '새로고침 중...' : '새로고침'}</button></div></section>
      <aside className="agenda"><div className="eyebrow">THE DAY AHEAD</div><div className="agenda-date"><div className="big-day">{Number(selected.slice(8))}</div><div><h2>{new Date(`${selected}T12:00:00`).toLocaleDateString('ko-KR', { weekday: 'long' })}</h2><p>{selected.replaceAll('-', '.')}</p></div></div><div className="agenda-section"><div className="section-title"><h3>일정 <span>{dayEvents.length}</span></h3><button aria-label="일정 추가" onClick={() => setEditor({ kind: 'event', value: { title: '', date: selected, start_time: '09:00', end_time: '10:00', all_day: false } })}>＋</button></div>{dayEvents.length ? dayEvents.map(e => <button className="agenda-event" key={e.calendar_id + e.id} onClick={() => editEvent(e)}><span className="event-line" style={{ background: e.color || '#8c9cff' }}/><span className="event-time">{eventTime(e)}</span><span className="event-title">{e.title}</span></button>) : <p className="empty">예정된 일정이 없어요.</p>}</div><div className="agenda-section tasks-section"><div className="section-title"><h3>할 일 <span>{dayTasks.length}</span></h3><button aria-label="할 일 추가" onClick={() => setEditor({ kind: 'task', value: { list_id: defaultList, title: '', notes: '', due: selected } })}>＋</button></div>{dayTasks.length ? dayTasks.map(t => <div className="agenda-task" key={t.list_id + t.id}><button className="check" aria-label="완료" onClick={() => mutate('complete_task', { listId: t.list_id, taskId: t.id, completed: true })}>○</button><button className="task-title" onClick={() => setEditor({ kind: 'task', value: { id: t.id, list_id: t.list_id, title: t.title, notes: t.notes, due: t.due ? dateOf(t.due) : '' } })}>{t.title}</button></div>) : <p className="empty">남은 할 일이 없어요.</p>}</div>{signedIn && calendars.length > 0 && <details className="calendar-visibility" open><summary>표시할 캘린더</summary><div className="calendar-options">{calendars.map(cal => <label key={cal.id} title={cal.title}><input type="checkbox" checked={!settings?.hidden_calendar_ids.includes(cal.id)} style={{ accentColor: cal.color }} onChange={e => toggleCalendar(cal.id, e.target.checked)}/><span>{cal.title}</span></label>)}</div></details>}<div className="aside-foot">{!signedIn ? <button className="primary" onClick={login}>Google 계정 연결 →</button> : <span>Google Calendar + Tasks</span>}</div></aside></main>}
    {editor && <div className="modal-backdrop" onMouseDown={() => setEditor(null)}><form className="modal" onMouseDown={e => e.stopPropagation()} onSubmit={e => { e.preventDefault(); saveEditor(); }}><div className="eyebrow">{editor.kind === 'event' ? 'CALENDAR EVENT' : 'GOOGLE TASK'}</div><h2>{editor.value.id ? '수정하기' : editor.kind === 'event' ? '새 일정' : '새 할 일'}</h2><label>제목<input autoFocus required value={editor.value.title} onChange={e => setEditor({ ...editor, value: { ...editor.value, title: e.target.value } } as typeof editor)}/></label>{editor.kind === 'event' ? <><label>날짜<input type="date" required value={editor.value.date} onChange={e => setEditor({ kind: 'event', value: { ...editor.value, date: e.target.value } })}/></label><label className="inline"><input type="checkbox" checked={editor.value.all_day} onChange={e => setEditor({ kind: 'event', value: { ...editor.value, all_day: e.target.checked } })}/> 종일 일정</label>{!editor.value.all_day && <div className="time-row"><label>시작<input type="time" value={editor.value.start_time} onChange={e => setEditor({ kind: 'event', value: { ...editor.value, start_time: e.target.value } })}/></label><label>종료<input type="time" value={editor.value.end_time} onChange={e => setEditor({ kind: 'event', value: { ...editor.value, end_time: e.target.value } })}/></label></div>}</> : <><label>마감일<input type="date" value={editor.value.due} onChange={e => setEditor({ kind: 'task', value: { ...editor.value, due: e.target.value } })}/></label><label>목록<select disabled={Boolean(editor.value.id)} value={editor.value.list_id} onChange={e => setEditor({ kind: 'task', value: { ...editor.value, list_id: e.target.value } })}>{data.task_lists.map(l => <option key={l.id} value={l.id}>{l.title}</option>)}</select></label><label>메모<textarea value={editor.value.notes} onChange={e => setEditor({ kind: 'task', value: { ...editor.value, notes: e.target.value } })}/></label></>}<div className="modal-actions">{editor.value.id && <button type="button" className="danger" onClick={() => editor.kind === 'event' ? mutate('delete_event', { calendarId: editor.value.calendar_id, eventId: editor.value.id }) : mutate('delete_task', { listId: editor.value.list_id, taskId: editor.value.id })}>삭제</button>}<button type="button" className="secondary" onClick={() => setEditor(null)}>취소</button><button className="primary" disabled={busy} type="submit">저장</button></div></form></div>}
  </div>;
}

createRoot(document.getElementById('root')!).render(<App />);
