import { useCallback, useEffect, useMemo, useState } from 'react';
import {
  Alert,
  AppBar,
  Box,
  Button,
  Chip,
  CircularProgress,
  Container,
  Divider,
  FormControlLabel,
  IconButton,
  InputAdornment,
  Link,
  Paper,
  Stack,
  Switch,
  Tab,
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableRow,
  Tabs,
  TextField,
  Toolbar,
  Tooltip,
  Typography
} from '@mui/material';
import { alpha } from '@mui/material/styles';
import {
  CircleCheck,
  CircleOff,
  KeyRound,
  LogOut,
  Play,
  Plus,
  RefreshCw,
  Save,
  Send,
  Trash2,
  Unplug
} from 'lucide-react';
import {
  AdResult,
  Dashboard,
  KeywordRule,
  Settings,
  PanelLog,
  TelegramAuthResponse,
  apiFetch
} from './api';

const emptySettings: Settings = {
  enabled: true,
  interval_seconds: 5,
  keywords: [],
  keyword_rules: [],
  channels: [],
  blacklist_channels: [],
  whitelist_channels: [],
  order_quantity: 100,
  max_results: 500
};

function App() {
  const [token, setToken] = useState(() => localStorage.getItem('vipads_token'));
  const [loginUsername, setLoginUsername] = useState('Izzatillo');
  const [loginPassword, setLoginPassword] = useState('');
  const [dashboard, setDashboard] = useState<Dashboard | null>(null);
  const [settings, setSettings] = useState<Settings>(emptySettings);
  const [tab, setTab] = useState(0);
  const [loading, setLoading] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [keywordInput, setKeywordInput] = useState('');
  const [keywordIntervalInput, setKeywordIntervalInput] = useState('5');
  const [channelInput, setChannelInput] = useState('');
  const [blacklistInput, setBlacklistInput] = useState('');
  const [whitelistInput, setWhitelistInput] = useState('');
  const [tgApiId, setTgApiId] = useState('');
  const [tgApiHash, setTgApiHash] = useState('');
  const [tgPhone, setTgPhone] = useState('');
  const [tgCode, setTgCode] = useState('');
  const [tgPassword, setTgPassword] = useState('');

  const refresh = useCallback(async () => {
    if (!token) return;
    setLoading(true);
    try {
      const data = await apiFetch<Dashboard>('/dashboard', token);
      setDashboard(data);
      setSettings(normalizeSettings(data.settings));
      setTgApiId(data.telegram.api_id ? String(data.telegram.api_id) : '');
      setTgPhone(data.telegram.phone ?? '');
      setError(null);
    } catch (err) {
      const message = err instanceof Error ? err.message : 'Xatolik';
      setError(message);
      if (message.includes('Avtorizatsiya')) {
        localStorage.removeItem('vipads_token');
        setToken(null);
      }
    } finally {
      setLoading(false);
    }
  }, [token]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  useEffect(() => {
    if (!token) return;
    const seconds = Math.max(2, settings.interval_seconds || 5);
    const id = window.setInterval(refresh, seconds * 1000);
    return () => window.clearInterval(id);
  }, [token, refresh, settings.interval_seconds]);

  const saveToken = (value: string | null) => {
    if (value) {
      localStorage.setItem('vipads_token', value);
    } else {
      localStorage.removeItem('vipads_token');
    }
    setToken(value);
  };

  const handleLogin = async () => {
    setBusy(true);
    setError(null);
    try {
      const data = await apiFetch<{ token: string }>('/auth/login', null, {
        method: 'POST',
        body: JSON.stringify({ username: loginUsername, password: loginPassword })
      });
      saveToken(data.token);
      setLoginPassword('');
      setNotice('Kirish muvaffaqiyatli');
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Login xato');
    } finally {
      setBusy(false);
    }
  };

  const saveSettings = async () => {
    setBusy(true);
    setError(null);
    try {
      const clean = await apiFetch<Settings>('/settings', token, {
        method: 'PUT',
        body: JSON.stringify(settings)
      });
      setSettings(normalizeSettings(clean));
      setNotice('Sozlamalar saqlandi');
      await refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Saqlash xato');
    } finally {
      setBusy(false);
    }
  };

  const syncKeywordSettings = (rules: KeywordRule[]) => ({
    keyword_rules: rules,
    keywords: rules
      .filter((rule) => rule.enabled && rule.text.trim())
      .map((rule) => rule.text.trim())
  });

  const addKeywordRules = () => {
    const items = keywordInput
      .split(',')
      .map((item) => item.trim())
      .filter(Boolean);
    if (!items.length) return;

    const interval = clampNumber(Number(keywordIntervalInput), 2, 86400, 5);
    setSettings((current) => {
      const existing = new Set(current.keyword_rules.map((rule) => rule.text.toLowerCase()));
      const nextRules = [...current.keyword_rules];

      for (const item of items) {
        if (!existing.has(item.toLowerCase())) {
          nextRules.push({
            text: item,
            interval_seconds: interval,
            enabled: true,
            last_checked_at: null,
            next_check_at: null
          });
          existing.add(item.toLowerCase());
        }
      }

      return {
        ...current,
        ...syncKeywordSettings(nextRules)
      };
    });
    setKeywordInput('');
  };

  const updateKeywordRule = (index: number, patch: Partial<KeywordRule>) => {
    setSettings((current) => {
      const nextRules = current.keyword_rules.map((rule, ruleIndex) =>
        ruleIndex === index
          ? {
              ...rule,
              ...patch,
              interval_seconds:
                patch.interval_seconds === undefined
                  ? rule.interval_seconds
                  : clampNumber(patch.interval_seconds, 2, 86400, 5)
            }
          : rule
      );

      return {
        ...current,
        ...syncKeywordSettings(nextRules)
      };
    });
  };

  const removeKeywordRule = (index: number) => {
    setSettings((current) => {
      const nextRules = current.keyword_rules.filter((_, ruleIndex) => ruleIndex !== index);
      return {
        ...current,
        ...syncKeywordSettings(nextRules)
      };
    });
  };

  const addListItem = (
    field: 'channels' | 'blacklist_channels' | 'whitelist_channels',
    value: string
  ) => {
    const items = value
      .split(',')
      .map((item) => item.trim())
      .filter(Boolean);
    if (!items.length) return;
    setSettings((current) => ({
      ...current,
      [field]: Array.from(new Set([...current[field], ...items]))
    }));
    if (field === 'channels') setChannelInput('');
    if (field === 'blacklist_channels') setBlacklistInput('');
    if (field === 'whitelist_channels') setWhitelistInput('');
  };

  const removeListItem = (
    field: 'channels' | 'blacklist_channels' | 'whitelist_channels',
    value: string
  ) => {
    setSettings((current) => ({
      ...current,
      [field]: current[field].filter((item) => item !== value)
    }));
  };

  const requestCode = async () => {
    setBusy(true);
    setError(null);
    try {
      const data = await apiFetch<TelegramAuthResponse>('/telegram/request-code', token, {
        method: 'POST',
        body: JSON.stringify({
          api_id: Number(tgApiId),
          api_hash: tgApiHash,
          phone: tgPhone
        })
      });
      setNotice(data.message);
      await refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Telegram login xato');
    } finally {
      setBusy(false);
    }
  };

  const verifyTelegram = async () => {
    setBusy(true);
    setError(null);
    try {
      const data = await apiFetch<TelegramAuthResponse>('/telegram/verify-code', token, {
        method: 'POST',
        body: JSON.stringify({
          code: tgCode || null,
          password: tgPassword || null
        })
      });
      setNotice(data.message);
      if (data.connected) {
        setTgCode('');
        setTgPassword('');
        setTgApiHash('');
      }
      await refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Tasdiqlash xato');
    } finally {
      setBusy(false);
    }
  };

  const disconnectTelegram = async () => {
    setBusy(true);
    setError(null);
    try {
      await apiFetch('/telegram/disconnect', token, { method: 'POST' });
      setNotice('Userbot uzildi');
      await refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Uzish xato');
    } finally {
      setBusy(false);
    }
  };

  const runScan = async () => {
    setBusy(true);
    setError(null);
    try {
      const data = await apiFetch<{ message: string }>('/scan/run', token, {
        method: 'POST'
      });
      setNotice(data.message);
      await refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Scan xato');
    } finally {
      setBusy(false);
    }
  };

  const clearResults = async () => {
    setBusy(true);
    setError(null);
    try {
      await apiFetch('/results', token, { method: 'DELETE' });
      setNotice('Natijalar tozalandi');
      await refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Tozalash xato');
    } finally {
      setBusy(false);
    }
  };

  const clearLogs = async () => {
    setBusy(true);
    setError(null);
    try {
      await apiFetch('/logs', token, { method: 'DELETE' });
      setNotice('Loglar tozalandi');
      await refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Log tozalash xato');
    } finally {
      setBusy(false);
    }
  };

  if (!token) {
    return (
      <Box className="panel-shell" sx={{ minHeight: '100vh', display: 'grid', placeItems: 'center', p: 2 }}>
        <Paper sx={{ width: '100%', maxWidth: 420, p: { xs: 2.5, sm: 4 }, borderTop: '4px solid #FFC107' }}>
          <Stack spacing={2.5}>
            <Box>
              <Typography variant="h4" color="primary">VIP Ads</Typography>
              <Typography color="text.secondary">Admin panel</Typography>
            </Box>
            {error && <Alert severity="error">{error}</Alert>}
            <TextField
              label="Login"
              value={loginUsername}
              onChange={(event) => setLoginUsername(event.target.value)}
              fullWidth
            />
            <TextField
              label="Parol"
              type="password"
              value={loginPassword}
              onChange={(event) => setLoginPassword(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === 'Enter') handleLogin();
              }}
              fullWidth
            />
            <Button
              variant="contained"
              color="primary"
              onClick={handleLogin}
              disabled={busy}
              startIcon={busy ? <CircularProgress size={16} color="inherit" /> : <KeyRound size={18} />}
            >
              Kirish
            </Button>
          </Stack>
        </Paper>
      </Box>
    );
  }

  return (
    <Box className="panel-shell">
      <AppBar position="static" elevation={0} className="top-band">
        <Toolbar sx={{ gap: 2, flexWrap: 'wrap', py: 1 }}>
          <Box sx={{ flex: 1, minWidth: 220 }}>
            <Typography variant="h5">VIP Ads</Typography>
            <Typography variant="body2" sx={{ opacity: 0.8 }}>izzatillo-aka.vipads.uz</Typography>
          </Box>
          <StatusChip
            ok={Boolean(dashboard?.status.telegram_connected)}
            label={dashboard?.status.telegram_connected ? 'Userbot ulangan' : 'Userbot ulanmagan'}
          />
          <StatusChip
            ok={Boolean(settings.enabled)}
            label={settings.enabled ? `Har ${settings.interval_seconds}s` : 'Skaner o\'chiq'}
          />
          <Tooltip title="Yangilash">
            <IconButton color="inherit" onClick={refresh} disabled={loading}>
              <RefreshCw size={20} />
            </IconButton>
          </Tooltip>
          <Tooltip title="Chiqish">
            <IconButton color="inherit" onClick={() => saveToken(null)}>
              <LogOut size={20} />
            </IconButton>
          </Tooltip>
        </Toolbar>
      </AppBar>

      <Container maxWidth="xl" sx={{ py: 3 }}>
        <Stack spacing={2.5}>
          {(error || notice || dashboard?.status.last_error) && (
            <Stack spacing={1}>
              {error && <Alert severity="error" onClose={() => setError(null)}>{error}</Alert>}
              {dashboard?.status.last_error && (
                <Alert severity="warning">{dashboard.status.last_error}</Alert>
              )}
              {notice && <Alert severity="success" onClose={() => setNotice(null)}>{notice}</Alert>}
            </Stack>
          )}

          <StatsBar dashboard={dashboard} loading={loading} />

          <Paper sx={{ overflow: 'hidden' }}>
            <Tabs
              value={tab}
              onChange={(_, value) => setTab(value)}
              variant="scrollable"
              scrollButtons="auto"
              sx={{
                borderBottom: 1,
                borderColor: 'divider',
                '& .MuiTab-root': { minHeight: 52, fontWeight: 800 }
              }}
            >
              <Tab label="Sozlamalar" />
              <Tab label="Userbot" />
              <Tab label="Natijalar" />
              <Tab label="Loglar" />
            </Tabs>

            <Box sx={{ p: { xs: 2, md: 3 } }}>
              {tab === 0 && (
                <SettingsPanel
                  settings={settings}
                  setSettings={setSettings}
                  keywordInput={keywordInput}
                  setKeywordInput={setKeywordInput}
                  keywordIntervalInput={keywordIntervalInput}
                  setKeywordIntervalInput={setKeywordIntervalInput}
                  channelInput={channelInput}
                  setChannelInput={setChannelInput}
                  blacklistInput={blacklistInput}
                  setBlacklistInput={setBlacklistInput}
                  whitelistInput={whitelistInput}
                  setWhitelistInput={setWhitelistInput}
                  addKeywordRules={addKeywordRules}
                  updateKeywordRule={updateKeywordRule}
                  removeKeywordRule={removeKeywordRule}
                  addListItem={addListItem}
                  removeListItem={removeListItem}
                  saveSettings={saveSettings}
                  busy={busy}
                />
              )}
              {tab === 1 && (
                <TelegramPanel
                  dashboard={dashboard}
                  tgApiId={tgApiId}
                  setTgApiId={setTgApiId}
                  tgApiHash={tgApiHash}
                  setTgApiHash={setTgApiHash}
                  tgPhone={tgPhone}
                  setTgPhone={setTgPhone}
                  tgCode={tgCode}
                  setTgCode={setTgCode}
                  tgPassword={tgPassword}
                  setTgPassword={setTgPassword}
                  requestCode={requestCode}
                  verifyTelegram={verifyTelegram}
                  disconnectTelegram={disconnectTelegram}
                  busy={busy}
                />
              )}
              {tab === 2 && (
                <ResultsPanel
                  results={dashboard?.results ?? []}
                  runScan={runScan}
                  clearResults={clearResults}
                  busy={busy}
                />
              )}
              {tab === 3 && (
                <LogsPanel
                  logs={dashboard?.logs ?? []}
                  clearLogs={clearLogs}
                  busy={busy}
                />
              )}
            </Box>
          </Paper>
        </Stack>
      </Container>
    </Box>
  );
}

function StatusChip({ ok, label }: { ok: boolean; label: string }) {
  return (
    <Chip
      icon={ok ? <CircleCheck size={16} /> : <CircleOff size={16} />}
      label={label}
      color={ok ? 'secondary' : 'default'}
      sx={{ fontWeight: 800, maxWidth: '100%' }}
    />
  );
}

function StatsBar({ dashboard, loading }: { dashboard: Dashboard | null; loading: boolean }) {
  const status = dashboard?.status;
  const items = [
    ['Natijalar', String(status?.total_results ?? 0)],
    ['Loglar', String(status?.total_logs ?? 0)],
    ['Oxirgi scan', formatDate(status?.last_run_at)],
    ['Keyingi scan', formatDate(status?.next_run_at)],
    ['Holat', status?.scanning ? 'Ishlayapti' : loading ? 'Yuklanmoqda' : 'Tayyor']
  ];

  return (
    <Box
      sx={{
        display: 'grid',
        gridTemplateColumns: { xs: '1fr 1fr', md: 'repeat(5, 1fr)' },
        gap: 1.5
      }}
    >
      {items.map(([label, value]) => (
        <Paper key={label} sx={{ p: 2, borderLeft: '4px solid #FFC107' }}>
          <Typography variant="caption" color="text.secondary" sx={{ fontWeight: 800 }}>
            {label}
          </Typography>
          <Typography variant="h6" className="text-clamp">
            {value}
          </Typography>
        </Paper>
      ))}
    </Box>
  );
}

type SettingsPanelProps = {
  settings: Settings;
  setSettings: (settings: Settings | ((current: Settings) => Settings)) => void;
  keywordInput: string;
  setKeywordInput: (value: string) => void;
  keywordIntervalInput: string;
  setKeywordIntervalInput: (value: string) => void;
  channelInput: string;
  setChannelInput: (value: string) => void;
  blacklistInput: string;
  setBlacklistInput: (value: string) => void;
  whitelistInput: string;
  setWhitelistInput: (value: string) => void;
  addKeywordRules: () => void;
  updateKeywordRule: (index: number, patch: Partial<KeywordRule>) => void;
  removeKeywordRule: (index: number) => void;
  addListItem: (
    field: 'channels' | 'blacklist_channels' | 'whitelist_channels',
    value: string
  ) => void;
  removeListItem: (
    field: 'channels' | 'blacklist_channels' | 'whitelist_channels',
    value: string
  ) => void;
  saveSettings: () => void;
  busy: boolean;
};

function SettingsPanel(props: SettingsPanelProps) {
  const {
    settings,
    setSettings,
    keywordInput,
    setKeywordInput,
    keywordIntervalInput,
    setKeywordIntervalInput,
    channelInput,
    setChannelInput,
    blacklistInput,
    setBlacklistInput,
    whitelistInput,
    setWhitelistInput,
    addKeywordRules,
    updateKeywordRule,
    removeKeywordRule,
    addListItem,
    removeListItem,
    saveSettings,
    busy
  } = props;

  return (
    <Stack spacing={3}>
      <Box sx={{ display: 'grid', gridTemplateColumns: { xs: '1fr', md: '220px 1fr 1fr 1fr' }, gap: 2 }}>
        <Paper variant="outlined" sx={{ p: 2 }}>
          <FormControlLabel
            control={
              <Switch
                checked={settings.enabled}
                onChange={(event) => setSettings((current) => ({ ...current, enabled: event.target.checked }))}
              />
            }
            label={settings.enabled ? 'Skaner yoqilgan' : 'Skaner o\'chiq'}
          />
        </Paper>
        <TextField
          label="Interval sekund"
          type="number"
          value={settings.interval_seconds}
          onChange={(event) =>
            setSettings((current) => ({ ...current, interval_seconds: Number(event.target.value) }))
          }
          slotProps={{ htmlInput: { min: 2, max: 3600 } }}
          fullWidth
        />
        <TextField
          label="Quality"
          type="number"
          value={settings.order_quantity}
          onChange={(event) =>
            setSettings((current) => ({ ...current, order_quantity: Number(event.target.value) }))
          }
          slotProps={{ htmlInput: { min: 1, max: 1000000 } }}
          fullWidth
        />
        <TextField
          label="Maksimal natija"
          type="number"
          value={settings.max_results}
          onChange={(event) =>
            setSettings((current) => ({ ...current, max_results: Number(event.target.value) }))
          }
          slotProps={{ htmlInput: { min: 50, max: 5000 } }}
          fullWidth
        />
      </Box>

      <KeywordRulesEditor
        rules={settings.keyword_rules}
        keywordInput={keywordInput}
        setKeywordInput={setKeywordInput}
        intervalInput={keywordIntervalInput}
        setIntervalInput={setKeywordIntervalInput}
        onAdd={addKeywordRules}
        onUpdate={updateKeywordRule}
        onRemove={removeKeywordRule}
      />

      <ListEditor
        title="Kanallar"
        placeholder="@kanal yoki t.me/kanal"
        value={channelInput}
        onChange={setChannelInput}
        items={settings.channels}
        onAdd={() => addListItem('channels', channelInput)}
        onRemove={(item) => removeListItem('channels', item)}
      />

      <Box sx={{ display: 'grid', gridTemplateColumns: { xs: '1fr', md: '1fr 1fr' }, gap: 2 }}>
        <ListEditor
          title="2-list: qora ro'yxat"
          placeholder="@kanal yoki t.me/kanal"
          value={blacklistInput}
          onChange={setBlacklistInput}
          items={settings.blacklist_channels}
          onAdd={() => addListItem('blacklist_channels', blacklistInput)}
          onRemove={(item) => removeListItem('blacklist_channels', item)}
        />

        <ListEditor
          title="3-list: oq ro'yxat"
          placeholder="@kanal yoki t.me/kanal"
          value={whitelistInput}
          onChange={setWhitelistInput}
          items={settings.whitelist_channels}
          onAdd={() => addListItem('whitelist_channels', whitelistInput)}
          onRemove={(item) => removeListItem('whitelist_channels', item)}
        />
      </Box>

      <Box>
        <Button variant="contained" onClick={saveSettings} disabled={busy} startIcon={<Save size={18} />}>
          Saqlash
        </Button>
      </Box>
    </Stack>
  );
}

function KeywordRulesEditor({
  rules,
  keywordInput,
  setKeywordInput,
  intervalInput,
  setIntervalInput,
  onAdd,
  onUpdate,
  onRemove
}: {
  rules: KeywordRule[];
  keywordInput: string;
  setKeywordInput: (value: string) => void;
  intervalInput: string;
  setIntervalInput: (value: string) => void;
  onAdd: () => void;
  onUpdate: (index: number, patch: Partial<KeywordRule>) => void;
  onRemove: (index: number) => void;
}) {
  return (
    <Stack spacing={1.5}>
      <Typography variant="h6">Keylar ro'yxati</Typography>
      <Box
        sx={{
          display: 'grid',
          gridTemplateColumns: { xs: '1fr', sm: '1fr 160px auto' },
          gap: 1
        }}
      >
        <TextField
          value={keywordInput}
          onChange={(event) => setKeywordInput(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === 'Enter') onAdd();
          }}
          placeholder="masalan: avto, kredit"
          fullWidth
        />
        <TextField
          label="Kutish"
          value={intervalInput}
          onChange={(event) => setIntervalInput(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === 'Enter') onAdd();
          }}
          type="number"
          slotProps={{
            htmlInput: { min: 2, max: 86400 },
            input: { endAdornment: <InputAdornment position="end">s</InputAdornment> }
          }}
          fullWidth
        />
        <Button variant="outlined" onClick={onAdd} startIcon={<Plus size={18} />} sx={{ minWidth: 118 }}>
          Qo'shish
        </Button>
      </Box>

      <Box sx={{ overflowX: 'auto' }}>
        <Table size="small" sx={{ minWidth: 760 }}>
          <TableHead>
            <TableRow>
              <TableCell sx={{ width: 88 }}>Holat</TableCell>
              <TableCell>Key</TableCell>
              <TableCell sx={{ width: 150 }}>Kutish</TableCell>
              <TableCell sx={{ width: 160 }}>Oxirgi</TableCell>
              <TableCell sx={{ width: 160 }}>Keyingi</TableCell>
              <TableCell align="right" sx={{ width: 72 }} />
            </TableRow>
          </TableHead>
          <TableBody>
            {rules.map((rule, index) => (
              <TableRow key={`${rule.text}-${index}`} hover>
                <TableCell>
                  <Switch
                    checked={rule.enabled}
                    onChange={(event) => onUpdate(index, { enabled: event.target.checked })}
                  />
                </TableCell>
                <TableCell>
                  <TextField
                    value={rule.text}
                    onChange={(event) => onUpdate(index, { text: event.target.value })}
                    size="small"
                    fullWidth
                  />
                </TableCell>
                <TableCell>
                  <TextField
                    value={rule.interval_seconds}
                    onChange={(event) => onUpdate(index, { interval_seconds: Number(event.target.value) })}
                    type="number"
                    size="small"
                    slotProps={{
                      htmlInput: { min: 2, max: 86400 },
                      input: { endAdornment: <InputAdornment position="end">s</InputAdornment> }
                    }}
                    fullWidth
                  />
                </TableCell>
                <TableCell sx={{ whiteSpace: 'nowrap' }}>{formatDate(rule.last_checked_at)}</TableCell>
                <TableCell sx={{ whiteSpace: 'nowrap' }}>{formatDate(rule.next_check_at)}</TableCell>
                <TableCell align="right">
                  <Tooltip title="O'chirish">
                    <IconButton color="error" onClick={() => onRemove(index)}>
                      <Trash2 size={18} />
                    </IconButton>
                  </Tooltip>
                </TableCell>
              </TableRow>
            ))}
            {!rules.length && (
              <TableRow>
                <TableCell colSpan={6}>
                  <Box
                    sx={{
                      p: 3,
                      textAlign: 'center',
                      bgcolor: (theme) => alpha(theme.palette.primary.main, 0.04)
                    }}
                  >
                    <Typography color="text.secondary">Bo'sh</Typography>
                  </Box>
                </TableCell>
              </TableRow>
            )}
          </TableBody>
        </Table>
      </Box>
    </Stack>
  );
}

function ListEditor({
  title,
  placeholder,
  value,
  onChange,
  items,
  onAdd,
  onRemove
}: {
  title: string;
  placeholder: string;
  value: string;
  onChange: (value: string) => void;
  items: string[];
  onAdd: () => void;
  onRemove: (item: string) => void;
}) {
  return (
    <Stack spacing={1.5}>
      <Typography variant="h6">{title}</Typography>
      <Stack direction={{ xs: 'column', sm: 'row' }} spacing={1}>
        <TextField
          value={value}
          onChange={(event) => onChange(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === 'Enter') onAdd();
          }}
          placeholder={placeholder}
          fullWidth
        />
        <Button variant="outlined" onClick={onAdd} startIcon={<Plus size={18} />} sx={{ minWidth: 110 }}>
          Qo'shish
        </Button>
      </Stack>
      <Stack direction="row" sx={{ gap: 1, flexWrap: 'wrap' }}>
        {items.map((item) => (
          <Chip
            key={item}
            label={item}
            onDelete={() => onRemove(item)}
            sx={{ maxWidth: '100%' }}
          />
        ))}
        {!items.length && <Typography color="text.secondary">Bo'sh</Typography>}
      </Stack>
    </Stack>
  );
}

type TelegramPanelProps = {
  dashboard: Dashboard | null;
  tgApiId: string;
  setTgApiId: (value: string) => void;
  tgApiHash: string;
  setTgApiHash: (value: string) => void;
  tgPhone: string;
  setTgPhone: (value: string) => void;
  tgCode: string;
  setTgCode: (value: string) => void;
  tgPassword: string;
  setTgPassword: (value: string) => void;
  requestCode: () => void;
  verifyTelegram: () => void;
  disconnectTelegram: () => void;
  busy: boolean;
};

function TelegramPanel(props: TelegramPanelProps) {
  const {
    dashboard,
    tgApiId,
    setTgApiId,
    tgApiHash,
    setTgApiHash,
    tgPhone,
    setTgPhone,
    tgCode,
    setTgCode,
    tgPassword,
    setTgPassword,
    requestCode,
    verifyTelegram,
    disconnectTelegram,
    busy
  } = props;
  const waitingFor = dashboard?.status.login_waiting_for;
  const connected = Boolean(dashboard?.status.telegram_connected);

  return (
    <Stack spacing={3}>
      <Box sx={{ display: 'grid', gridTemplateColumns: { xs: '1fr', md: '1fr 1fr 1fr' }, gap: 2 }}>
        <TextField
          label="API ID"
          value={tgApiId}
          onChange={(event) => setTgApiId(event.target.value)}
          type="number"
          fullWidth
        />
        <TextField
          label="API hash"
          value={tgApiHash}
          onChange={(event) => setTgApiHash(event.target.value)}
          type="password"
          fullWidth
          slotProps={{
            input: {
              endAdornment: dashboard?.telegram.api_hash ? (
                <InputAdornment position="end">saqlangan</InputAdornment>
              ) : null
            }
          }}
        />
        <TextField
          label="Telefon"
          value={tgPhone}
          onChange={(event) => setTgPhone(event.target.value)}
          placeholder="+998..."
          fullWidth
        />
      </Box>

      <Stack direction={{ xs: 'column', sm: 'row' }} spacing={1}>
        <Button
          variant="contained"
          onClick={requestCode}
          disabled={busy}
          startIcon={<Send size={18} />}
        >
          Kod olish
        </Button>
        <Button
          variant="outlined"
          color="error"
          onClick={disconnectTelegram}
          disabled={busy || !connected}
          startIcon={<Unplug size={18} />}
        >
          Uzish
        </Button>
      </Stack>

      <Divider />

      <Box sx={{ display: 'grid', gridTemplateColumns: { xs: '1fr', md: '1fr 1fr auto' }, gap: 2, alignItems: 'center' }}>
        <TextField
          label="Kod"
          value={tgCode}
          onChange={(event) => setTgCode(event.target.value)}
          disabled={waitingFor === 'password'}
          fullWidth
        />
        <TextField
          label="2FA parol"
          value={tgPassword}
          onChange={(event) => setTgPassword(event.target.value)}
          type="password"
          fullWidth
        />
        <Button
          variant="contained"
          color="secondary"
          onClick={verifyTelegram}
          disabled={busy || connected}
          startIcon={<CircleCheck size={18} />}
          sx={{ minWidth: 140 }}
        >
          Tasdiqlash
        </Button>
      </Box>
    </Stack>
  );
}

function ResultsPanel({
  results,
  runScan,
  clearResults,
  busy
}: {
  results: AdResult[];
  runScan: () => void;
  clearResults: () => void;
  busy: boolean;
}) {
  const sorted = useMemo(() => results, [results]);

  return (
    <Stack spacing={2}>
      <Stack direction={{ xs: 'column', sm: 'row' }} spacing={1} sx={{ justifyContent: 'space-between' }}>
        <Typography variant="h6">Topilgan ads</Typography>
        <Stack direction="row" spacing={1}>
          <Button variant="contained" onClick={runScan} disabled={busy} startIcon={<Play size={18} />}>
            Hozir tekshirish
          </Button>
          <Button variant="outlined" color="error" onClick={clearResults} disabled={busy} startIcon={<Trash2 size={18} />}>
            Tozalash
          </Button>
        </Stack>
      </Stack>

      <Box sx={{ overflowX: 'auto' }}>
        <Table sx={{ minWidth: 1540 }}>
          <TableHead>
            <TableRow>
              <TableCell>Vaqt</TableCell>
              <TableCell>Kanal</TableCell>
              <TableCell>Target</TableCell>
              <TableCell>Kalit</TableCell>
              <TableCell>Sarlavha</TableCell>
              <TableCell>Matn</TableCell>
              <TableCell>Sponsor</TableCell>
              <TableCell>Qo'shimcha</TableCell>
              <TableCell>URL</TableCell>
              <TableCell>ID</TableCell>
            </TableRow>
          </TableHead>
          <TableBody>
            {sorted.map((item) => (
              <TableRow key={item.id} hover>
                <TableCell sx={{ whiteSpace: 'nowrap' }}>{formatDate(item.found_at)}</TableCell>
                <TableCell className="text-clamp">
                  <Typography sx={{ fontWeight: 800 }}>{item.channel_title || item.channel}</Typography>
                  <Typography variant="caption" color="text.secondary">@{item.channel}</Typography>
                </TableCell>
                <TableCell className="text-clamp">
                  {formatChannel(item.target_channel)}
                </TableCell>
                <TableCell>
                  <Stack direction="row" sx={{ gap: 0.5, flexWrap: 'wrap' }}>
                    {(item.matched_keywords.length ? item.matched_keywords : ['all']).map((keyword) => (
                      <Chip key={keyword} label={keyword} size="small" color="secondary" />
                    ))}
                  </Stack>
                </TableCell>
                <TableCell className="text-clamp" sx={{ maxWidth: 220 }}>{item.title}</TableCell>
                <TableCell className="text-clamp" sx={{ maxWidth: 340 }}>{item.message}</TableCell>
                <TableCell className="text-clamp" sx={{ maxWidth: 240 }}>
                  {item.sponsor_info || '-'}
                </TableCell>
                <TableCell className="text-clamp" sx={{ maxWidth: 240 }}>
                  {item.additional_info || '-'}
                </TableCell>
                <TableCell className="text-clamp" sx={{ maxWidth: 220 }}>
                  <Link href={item.url} target="_blank" rel="noreferrer">{item.url}</Link>
                  <Typography variant="caption" sx={{ display: 'block' }} color="text.secondary">
                    {item.button_text}
                  </Typography>
                  {item.recommended && (
                    <Chip label="recommended" size="small" color="secondary" sx={{ mt: 0.75 }} />
                  )}
                </TableCell>
                <TableCell className="text-clamp" sx={{ maxWidth: 180 }}>
                  <Typography variant="caption">{item.random_id_hex}</Typography>
                </TableCell>
              </TableRow>
            ))}
            {!sorted.length && (
              <TableRow>
                <TableCell colSpan={10}>
                  <Box
                    sx={{
                      p: 4,
                      textAlign: 'center',
                      bgcolor: (theme) => alpha(theme.palette.primary.main, 0.04)
                    }}
                  >
                    <Typography color="text.secondary">Natija yo'q</Typography>
                  </Box>
                </TableCell>
              </TableRow>
            )}
          </TableBody>
        </Table>
      </Box>
    </Stack>
  );
}

function LogsPanel({
  logs,
  clearLogs,
  busy
}: {
  logs: PanelLog[];
  clearLogs: () => void;
  busy: boolean;
}) {
  return (
    <Stack spacing={2}>
      <Stack direction={{ xs: 'column', sm: 'row' }} spacing={1} sx={{ justifyContent: 'space-between' }}>
        <Typography variant="h6">Admin log</Typography>
        <Button variant="outlined" color="error" onClick={clearLogs} disabled={busy} startIcon={<Trash2 size={18} />}>
          Tozalash
        </Button>
      </Stack>

      <Box sx={{ overflowX: 'auto' }}>
        <Table sx={{ minWidth: 1320 }}>
          <TableHead>
            <TableRow>
              <TableCell>Vaqt</TableCell>
              <TableCell>Holat</TableCell>
              <TableCell>Xabar</TableCell>
              <TableCell>Key</TableCell>
              <TableCell>Source</TableCell>
              <TableCell>Target</TableCell>
              <TableCell>Order</TableCell>
              <TableCell>Javob</TableCell>
            </TableRow>
          </TableHead>
          <TableBody>
            {logs.map((log) => (
              <TableRow key={log.id} hover>
                <TableCell sx={{ whiteSpace: 'nowrap' }}>{formatDate(log.created_at)}</TableCell>
                <TableCell>
                  <Chip
                    size="small"
                    label={log.level}
                    color={log.level === 'success' ? 'secondary' : log.level === 'error' ? 'error' : 'default'}
                  />
                </TableCell>
                <TableCell className="text-clamp" sx={{ maxWidth: 360 }}>
                  <Typography sx={{ fontWeight: 800 }}>{log.title}</Typography>
                  <Typography variant="body2">{log.message}</Typography>
                  {log.ad_url && (
                    <Link href={log.ad_url} target="_blank" rel="noreferrer" variant="caption">
                      {log.ad_url}
                    </Link>
                  )}
                </TableCell>
                <TableCell className="text-clamp">{log.keyword || '-'}</TableCell>
                <TableCell className="text-clamp">{log.source_channel || '-'}</TableCell>
                <TableCell className="text-clamp">{log.target_channel || '-'}</TableCell>
                <TableCell className="text-clamp" sx={{ maxWidth: 240 }}>
                  <Typography variant="body2">service: {log.service_id || '-'}</Typography>
                  <Typography variant="body2">quality: {log.quantity || '-'}</Typography>
                  <Typography variant="body2">order: {log.order_id || '-'}</Typography>
                  {log.order_link && (
                    <Link href={log.order_link} target="_blank" rel="noreferrer" variant="caption">
                      {log.order_link}
                    </Link>
                  )}
                </TableCell>
                <TableCell className="text-clamp" sx={{ maxWidth: 300 }}>
                  {log.raw_response || '-'}
                </TableCell>
              </TableRow>
            ))}
            {!logs.length && (
              <TableRow>
                <TableCell colSpan={8}>
                  <Box
                    sx={{
                      p: 4,
                      textAlign: 'center',
                      bgcolor: (theme) => alpha(theme.palette.primary.main, 0.04)
                    }}
                  >
                    <Typography color="text.secondary">Log yo'q</Typography>
                  </Box>
                </TableCell>
              </TableRow>
            )}
          </TableBody>
        </Table>
      </Box>
    </Stack>
  );
}

function formatDate(value?: string | null) {
  if (!value) return '-';
  return new Date(value).toLocaleString('uz-UZ', {
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit'
  });
}

function formatChannel(value?: string | null) {
  if (!value) return '-';
  if (value.startsWith('+') || value.startsWith('http')) return value;
  return `@${value.replace(/^@/, '')}`;
}

function normalizeSettings(settings: Settings): Settings {
  const keywordRules = settings.keyword_rules?.length
    ? settings.keyword_rules
    : settings.keywords.map((keyword) => ({
        text: keyword,
        interval_seconds: settings.interval_seconds || 5,
        enabled: true,
        last_checked_at: null,
        next_check_at: null
      }));

  return {
    ...settings,
    keyword_rules: keywordRules,
    blacklist_channels: settings.blacklist_channels ?? [],
    whitelist_channels: settings.whitelist_channels ?? [],
    order_quantity: settings.order_quantity || 100,
    keywords: keywordRules
      .filter((rule) => rule.enabled && rule.text.trim())
      .map((rule) => rule.text.trim())
  };
}

function clampNumber(value: number, min: number, max: number, fallback: number) {
  if (!Number.isFinite(value)) return fallback;
  return Math.min(max, Math.max(min, value));
}

export default App;
