import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
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
  Typography,
  useMediaQuery
} from '@mui/material';
import { alpha, useTheme, type Theme } from '@mui/material/styles';
import {
  CircleCheck,
  CircleOff,
  KeyRound,
  LogOut,
  Play,
  Plus,
  QrCode,
  RefreshCw,
  Save,
  Send,
  Trash2
} from 'lucide-react';
import QRCodeLib from 'qrcode';
import {
  AccountStatus,
  AdResult,
  ApiError,
  Dashboard,
  KeywordRule,
  KeywordStat,
  Settings,
  PanelLog,
  QrPollResponse,
  QrStartResponse,
  apiFetch
} from './api';

const emptySettings: Settings = {
  interval_seconds: 5,
  keywords: [],
  keyword_rules: [],
  channels: [],
  whitelist_channels: [],
  order_quantity: 100
};

function App() {
  const theme = useTheme();
  const isMobile = useMediaQuery(theme.breakpoints.down('md'));

  const [token, setToken] = useState(() => localStorage.getItem('vipads_token'));
  const [loginUsername, setLoginUsername] = useState('Feruz');
  const [loginPassword, setLoginPassword] = useState('');
  const [dashboard, setDashboard] = useState<Dashboard | null>(null);
  const [settings, setSettings] = useState<Settings>(emptySettings);
  const [tab, setTab] = useState(0);
  const [loading, setLoading] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [keywordInput, setKeywordInput] = useState('');
  const [keywordIntervalInput, setKeywordIntervalInput] = useState('5');
  const [keywordQuantityInput, setKeywordQuantityInput] = useState('100');
  const [whitelistInput, setWhitelistInput] = useState('');

  // Foydalanuvchi sozlamalarni tahrirlaganini kuzatamiz. Poll (har interval'da)
  // server nusxasini yuklab kelganda, agar tahrir saqlanmagan bo'lsa, uni
  // bosib ketmasligi uchun shu bayroqdan foydalanamiz.
  const dirtyRef = useRef(false);
  const initializedRef = useRef(false);
  // Har doim eng so'nggi sozlamalarni saqlaymiz, shunda avto-saqlashda eskirgan
  // qiymat yuborilmaydi.
  const settingsRef = useRef(settings);
  useEffect(() => {
    settingsRef.current = settings;
  }, [settings]);

  const markDirty = useCallback(() => {
    dirtyRef.current = true;
  }, []);

  const clearDirty = useCallback(() => {
    dirtyRef.current = false;
  }, []);

  const handleAuthError = useCallback((err: unknown): boolean => {
    if (err instanceof ApiError && err.status === 401) {
      localStorage.removeItem('vipads_token');
      setToken(null);
      setDashboard(null);
      initializedRef.current = false;
      return true;
    }
    return false;
  }, []);

  const refresh = useCallback(async () => {
    if (!token) return;
    setLoading(true);
    try {
      const data = await apiFetch<Dashboard>('/dashboard', token);
      setDashboard(data);
      // Saqlanmagan tahrir bo'lmasagina draft sozlamalarni serverdan yangilaymiz.
      if (!dirtyRef.current) {
        setSettings(normalizeSettings(data.settings));
      }
      setError(null);
    } catch (err) {
      if (!handleAuthError(err)) {
        setError(err instanceof Error ? err.message : 'Xatolik');
      }
    } finally {
      setLoading(false);
    }
  }, [token, handleAuthError]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  // Poll cadence serverdagi interval'ga bog'lanadi (draft maydonga emas), shuning
  // uchun interval maydonini yozayotganda timer qayta-qayta yaralmaydi.
  const serverInterval = dashboard?.settings.interval_seconds ?? 5;
  useEffect(() => {
    if (!token) return;
    const seconds = Math.max(2, serverInterval || 5);
    const id = window.setInterval(refresh, seconds * 1000);
    return () => window.clearInterval(id);
  }, [token, refresh, serverInterval]);

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
      initializedRef.current = false;
      saveToken(data.token);
      setLoginPassword('');
      setNotice('Kirish muvaffaqiyatli');
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Login xato');
    } finally {
      setBusy(false);
    }
  };

  const logout = async () => {
    try {
      await apiFetch('/auth/logout', token, { method: 'POST' });
    } catch {
      // chiqishda xatoni jim o'tkazib yuboramiz
    }
    initializedRef.current = false;
    setDashboard(null);
    saveToken(null);
  };

  const editSettings = useCallback(
    (updater: Settings | ((current: Settings) => Settings)) => {
      markDirty();
      setSettings((prev) => (typeof updater === 'function' ? updater(prev) : updater));
    },
    [markDirty]
  );

  // Avto-saqlash: butun sozlamalar obyektini serverga yuboradi (PUT /settings butun
  // obyektni oladi). Saqlash davomida dirty qoldiriladi, shunda poll draftni bosib
  // ketmaydi; muvaffaqiyatda tozalanadi.
  const commitSettings = useCallback(
    async (next: Settings, successMessage?: string) => {
      setError(null);
      setSaving(true);
      dirtyRef.current = true;
      try {
        await apiFetch<Settings>('/settings', token, {
          method: 'PUT',
          body: JSON.stringify(next)
        });
        clearDirty();
        if (successMessage) setNotice(successMessage);
      } catch (err) {
        if (!handleAuthError(err)) {
          setError(err instanceof Error ? err.message : 'Saqlash xato');
        }
      } finally {
        setSaving(false);
      }
    },
    [token, handleAuthError, clearDirty]
  );

  // Joriy (eng so'nggi) sozlamalarni saqlaydi — matn/raqam maydonlari fokusdan
  // chiqqanda (onBlur) chaqiriladi.
  const commitNow = useCallback(
    (successMessage?: string) => {
      void commitSettings(settingsRef.current, successMessage);
    },
    [commitSettings]
  );

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

    const base = settingsRef.current;
    const interval = clampNumber(Number(keywordIntervalInput), 2, 86400, 5);
    const quantity = clampNumber(Number(keywordQuantityInput), 1, 1000000, base.order_quantity || 100);
    const existing = new Set(base.keyword_rules.map((rule) => rule.text.toLowerCase()));
    const nextRules = [...base.keyword_rules];

    for (const item of items) {
      if (!existing.has(item.toLowerCase())) {
        nextRules.push({
          text: item,
          interval_seconds: interval,
          order_quantity: quantity,
          enabled: true,
          last_checked_at: null,
          next_check_at: null
        });
        existing.add(item.toLowerCase());
      }
    }

    const next = { ...base, ...syncKeywordSettings(nextRules) };
    setSettings(next);
    setKeywordInput('');
    void commitSettings(next, "Key qo'shildi");
  };

  // Matn/raqam maydonlari: faqat draftni yangilaydi (markDirty). Saqlash onBlur
  // (fokusdan chiqqanda) commitNow orqali sodir bo'ladi.
  const updateKeywordRule = (index: number, patch: Partial<KeywordRule>) => {
    editSettings((current) => {
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

  // Switch (yoqish/o'chirish) — darhol saqlanadi.
  const setRuleEnabled = (index: number, enabled: boolean) => {
    const nextRules = settingsRef.current.keyword_rules.map((rule, ruleIndex) =>
      ruleIndex === index ? { ...rule, enabled } : rule
    );
    const next = { ...settingsRef.current, ...syncKeywordSettings(nextRules) };
    setSettings(next);
    void commitSettings(next);
  };

  const removeKeywordRule = (index: number) => {
    const nextRules = settingsRef.current.keyword_rules.filter((_, ruleIndex) => ruleIndex !== index);
    const next = { ...settingsRef.current, ...syncKeywordSettings(nextRules) };
    setSettings(next);
    void commitSettings(next, "Key o'chirildi");
  };

  const addListItem = (field: 'whitelist_channels', value: string) => {
    const items = value
      .split(',')
      .map((item) => item.trim())
      .filter(Boolean);
    if (!items.length) return;
    const next = {
      ...settingsRef.current,
      [field]: Array.from(new Set([...settingsRef.current[field], ...items]))
    };
    setSettings(next);
    setWhitelistInput('');
    void commitSettings(next, "Ro'yxat saqlandi");
  };

  const removeListItem = (field: 'whitelist_channels', value: string) => {
    const next = {
      ...settingsRef.current,
      [field]: settingsRef.current[field].filter((item) => item !== value)
    };
    setSettings(next);
    void commitSettings(next, "Ro'yxat saqlandi");
  };

  const sendOrderNow = async (keyword: string) => {
    setBusy(true);
    setError(null);
    try {
      const data = await apiFetch<{ message: string }>('/order/send', token, {
        method: 'POST',
        body: JSON.stringify({ keyword })
      });
      setNotice(data.message);
      await refresh();
    } catch (err) {
      if (!handleAuthError(err)) {
        setError(err instanceof Error ? err.message : 'Order xato');
      }
    } finally {
      setBusy(false);
    }
  };

  const runScan = async (keyword?: string) => {
    setBusy(true);
    setError(null);
    try {
      const data = await apiFetch<{ message: string }>('/scan/run', token, {
        method: 'POST',
        body: JSON.stringify(keyword ? { keyword } : {})
      });
      setNotice(keyword ? `"${keyword}" tekshirildi: ${data.message}` : data.message);
      await refresh();
    } catch (err) {
      if (!handleAuthError(err)) {
        setError(err instanceof Error ? err.message : 'Scan xato');
      }
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
      if (!handleAuthError(err)) {
        setError(err instanceof Error ? err.message : 'Tozalash xato');
      }
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
      if (!handleAuthError(err)) {
        setError(err instanceof Error ? err.message : 'Log tozalash xato');
      }
    } finally {
      setBusy(false);
    }
  };

  if (!token) {
    return (
      <Box
        className="panel-shell"
        sx={{ minHeight: '100vh', display: 'grid', placeItems: 'center', p: 2 }}
      >
        <Paper
          sx={{ width: '100%', maxWidth: 420, p: { xs: 2.5, sm: 4 }, borderTop: '4px solid #FFC107' }}
        >
          <Stack spacing={2.5}>
            <Box>
              <Typography variant="h4" color="primary">
                VIP Ads
              </Typography>
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
              size="large"
              startIcon={busy ? <CircularProgress size={16} color="inherit" /> : <KeyRound size={18} />}
            >
              Kirish
            </Button>
          </Stack>
        </Paper>
      </Box>
    );
  }

  const connected = Boolean(dashboard?.status.telegram_connected);
  const activeKeys = (dashboard?.settings.keyword_rules ?? settings.keyword_rules).filter(
    (rule) => rule.enabled && rule.text.trim()
  ).length;

  return (
    <Box className="panel-shell" sx={{ pb: 4 }}>
      <AppBar position="sticky" elevation={0} className="top-band">
        <Toolbar sx={{ gap: 1, py: 1, minHeight: { xs: 56, sm: 64 } }}>
          <Box sx={{ flex: 1, minWidth: 0 }}>
            <Typography variant="h6" sx={{ lineHeight: 1.1 }}>
              VIP Ads
            </Typography>
            <Typography variant="caption" sx={{ opacity: 0.8 }} noWrap>
              avto-xujim-feruz-aka.vipads.uz
            </Typography>
          </Box>
          <Tooltip title="Yangilash">
            <span>
              <IconButton color="inherit" onClick={refresh} disabled={loading}>
                <RefreshCw size={20} />
              </IconButton>
            </span>
          </Tooltip>
          <Tooltip title="Chiqish">
            <IconButton color="inherit" onClick={logout}>
              <LogOut size={20} />
            </IconButton>
          </Tooltip>
        </Toolbar>
        <Box sx={{ px: 1.5, pb: 1.25, display: 'flex', gap: 1, flexWrap: 'wrap' }}>
          <StatusChip ok={connected} label={connected ? 'Userbot ulangan' : 'Userbot ulanmagan'} />
          <StatusChip
            ok={activeKeys > 0}
            label={activeKeys > 0 ? `${activeKeys} ta key ishlayapti` : "Aktiv key yo'q"}
          />
        </Box>
      </AppBar>

      <Container maxWidth="xl" sx={{ py: { xs: 2, md: 3 }, px: { xs: 1.5, sm: 2, md: 3 } }}>
        <Stack spacing={2}>
          {(error || notice || dashboard?.status.last_error) && (
            <Stack spacing={1}>
              {error && (
                <Alert severity="error" onClose={() => setError(null)}>
                  {error}
                </Alert>
              )}
              {dashboard?.status.last_error && (
                <Alert severity="warning">{dashboard.status.last_error}</Alert>
              )}
              {notice && (
                <Alert severity="success" onClose={() => setNotice(null)}>
                  {notice}
                </Alert>
              )}
            </Stack>
          )}

          <StatsBar dashboard={dashboard} loading={loading} />

          <Paper sx={{ overflow: 'hidden' }}>
            <Tabs
              value={tab}
              onChange={(_, value) => setTab(value)}
              variant="scrollable"
              scrollButtons="auto"
              allowScrollButtonsMobile
              sx={{
                borderBottom: 1,
                borderColor: 'divider',
                '& .MuiTab-root': { minHeight: 52, fontWeight: 800 }
              }}
            >
              <Tab label="Sozlamalar" />
              <Tab label="Userbot" />
              <Tab label="Natijalar" />
              <Tab label={`Loglar (${dashboard?.status.total_logs ?? 0})`} />
            </Tabs>

            <Box sx={{ p: { xs: 1.5, sm: 2, md: 3 } }}>
              {tab === 0 && (
                <SettingsPanel
                  settings={settings}
                  setSettings={editSettings}
                  saving={saving}
                  isMobile={isMobile}
                  commitNow={commitNow}
                  keywordInput={keywordInput}
                  setKeywordInput={setKeywordInput}
                  keywordIntervalInput={keywordIntervalInput}
                  setKeywordIntervalInput={setKeywordIntervalInput}
                  keywordQuantityInput={keywordQuantityInput}
                  setKeywordQuantityInput={setKeywordQuantityInput}
                  whitelistInput={whitelistInput}
                  setWhitelistInput={setWhitelistInput}
                  addKeywordRules={addKeywordRules}
                  updateKeywordRule={updateKeywordRule}
                  setRuleEnabled={setRuleEnabled}
                  removeKeywordRule={removeKeywordRule}
                  runKeyNow={(keyword) => void runScan(keyword)}
                  sendOrderNow={(keyword) => void sendOrderNow(keyword)}
                  addListItem={addListItem}
                  removeListItem={removeListItem}
                  busy={busy}
                />
              )}
              {tab === 1 && (
                <AccountsPanel
                  token={token}
                  accounts={dashboard?.accounts ?? []}
                  apiConfigured={Boolean(dashboard?.telegram.api_id) && Boolean(dashboard?.telegram.api_hash)}
                  savedApiId={dashboard?.telegram.api_id ?? null}
                  onRefresh={refresh}
                  onNotice={setNotice}
                  onError={setError}
                />
              )}
              {tab === 2 && (
                <ResultsPanel
                  results={dashboard?.results ?? []}
                  whitelist={dashboard?.settings.whitelist_channels ?? []}
                  stats={dashboard?.stats_24h ?? []}
                  runScan={() => void runScan()}
                  clearResults={clearResults}
                  busy={busy}
                  isMobile={isMobile}
                />
              )}
              {tab === 3 && (
                <LogsPanel logs={dashboard?.logs ?? []} clearLogs={clearLogs} busy={busy} isMobile={isMobile} />
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
      size="small"
      icon={ok ? <CircleCheck size={15} /> : <CircleOff size={15} />}
      label={label}
      color={ok ? 'secondary' : 'default'}
      sx={{
        fontWeight: 800,
        maxWidth: '100%',
        ...(!ok && {
          bgcolor: 'rgba(255,255,255,0.16)',
          color: '#fff',
          '& .MuiChip-icon': { color: 'rgba(255,255,255,0.85)' }
        })
      }}
    />
  );
}


function StatsBar({ dashboard, loading }: { dashboard: Dashboard | null; loading: boolean }) {
  const status = dashboard?.status;
  const activeKeys = (dashboard?.settings.keyword_rules ?? []).filter(
    (rule) => rule.enabled && rule.text.trim()
  ).length;
  const items: [string, string][] = [
    ['Aktiv keylar', String(activeKeys)],
    ['Natijalar', String(status?.total_results ?? 0)],
    ['Loglar', String(status?.total_logs ?? 0)],
    ['Oxirgi scan', formatDate(status?.last_run_at)],
    ['Keyingi scan', formatDate(status?.next_run_at)],
    [
      'Holat',
      status?.scanning ? 'Tekshiryapti' : loading ? 'Yuklanmoqda' : activeKeys > 0 ? 'Ishlayapti' : 'Kutmoqda'
    ]
  ];

  return (
    <Box
      sx={{
        display: 'grid',
        gridTemplateColumns: { xs: '1fr 1fr', sm: 'repeat(3, 1fr)', lg: 'repeat(6, 1fr)' },
        gap: { xs: 1, md: 1.5 }
      }}
    >
      {items.map(([label, value]) => (
        <Paper key={label} sx={{ p: { xs: 1.25, md: 2 }, borderLeft: '4px solid #FFC107' }}>
          <Typography variant="caption" color="text.secondary" sx={{ fontWeight: 800 }}>
            {label}
          </Typography>
          <Typography variant="h6" className="text-clamp" sx={{ fontSize: { xs: '1rem', md: '1.25rem' } }}>
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
  saving: boolean;
  isMobile: boolean;
  commitNow: (successMessage?: string) => void;
  keywordInput: string;
  setKeywordInput: (value: string) => void;
  keywordIntervalInput: string;
  setKeywordIntervalInput: (value: string) => void;
  keywordQuantityInput: string;
  setKeywordQuantityInput: (value: string) => void;
  whitelistInput: string;
  setWhitelistInput: (value: string) => void;
  addKeywordRules: () => void;
  updateKeywordRule: (index: number, patch: Partial<KeywordRule>) => void;
  setRuleEnabled: (index: number, enabled: boolean) => void;
  removeKeywordRule: (index: number) => void;
  runKeyNow: (keyword: string) => void;
  sendOrderNow: (keyword: string) => void;
  addListItem: (field: 'whitelist_channels', value: string) => void;
  removeListItem: (field: 'whitelist_channels', value: string) => void;
  busy: boolean;
};

function SettingsPanel(props: SettingsPanelProps) {
  const {
    settings,
    setSettings,
    saving,
    isMobile,
    commitNow,
    keywordInput,
    setKeywordInput,
    keywordIntervalInput,
    setKeywordIntervalInput,
    keywordQuantityInput,
    setKeywordQuantityInput,
    whitelistInput,
    setWhitelistInput,
    addKeywordRules,
    updateKeywordRule,
    setRuleEnabled,
    removeKeywordRule,
    runKeyNow,
    sendOrderNow,
    addListItem,
    removeListItem,
    busy
  } = props;

  return (
    <Stack spacing={3}>
      <Stack
        direction={{ xs: 'column', sm: 'row' }}
        spacing={1.5}
        sx={{ alignItems: { xs: 'stretch', sm: 'center' }, justifyContent: 'space-between' }}
      >
        <Stack direction="row" spacing={1} sx={{ alignItems: 'center' }}>
          <Save size={16} />
          <Typography variant="caption" color="text.secondary" sx={{ fontWeight: 700 }}>
            O'zgarishlar avtomatik saqlanadi
          </Typography>
          {saving && <CircularProgress size={14} />}
        </Stack>
        <TextField
          label="Skaner davri (sekund)"
          type="number"
          size="small"
          value={settings.interval_seconds}
          onChange={(event) =>
            setSettings((current) => ({ ...current, interval_seconds: Number(event.target.value) }))
          }
          onBlur={() => commitNow()}
          slotProps={{ htmlInput: { min: 2, max: 3600 } }}
          sx={{ width: { xs: '100%', sm: 200 } }}
        />
      </Stack>

      <KeywordRulesEditor
        rules={settings.keyword_rules}
        isMobile={isMobile}
        keywordInput={keywordInput}
        setKeywordInput={setKeywordInput}
        intervalInput={keywordIntervalInput}
        setIntervalInput={setKeywordIntervalInput}
        quantityInput={keywordQuantityInput}
        setQuantityInput={setKeywordQuantityInput}
        onAdd={addKeywordRules}
        onUpdate={updateKeywordRule}
        onCommit={commitNow}
        onToggle={setRuleEnabled}
        onRemove={removeKeywordRule}
        onRunKey={runKeyNow}
        onSendOrder={sendOrderNow}
        busy={busy}
      />

      <ListEditor
        title="Oq ro'yxat (bu kanallarga order yuborilmaydi, qolgan hammasiga yuboriladi)"
        placeholder="@kanal yoki t.me/kanal"
        value={whitelistInput}
        onChange={setWhitelistInput}
        items={settings.whitelist_channels}
        onAdd={() => addListItem('whitelist_channels', whitelistInput)}
        onRemove={(item) => removeListItem('whitelist_channels', item)}
      />
    </Stack>
  );
}

function KeywordRulesEditor({
  rules,
  isMobile,
  keywordInput,
  setKeywordInput,
  intervalInput,
  setIntervalInput,
  quantityInput,
  setQuantityInput,
  onAdd,
  onUpdate,
  onCommit,
  onToggle,
  onRemove,
  onRunKey,
  onSendOrder,
  busy
}: {
  rules: KeywordRule[];
  isMobile: boolean;
  keywordInput: string;
  setKeywordInput: (value: string) => void;
  intervalInput: string;
  setIntervalInput: (value: string) => void;
  quantityInput: string;
  setQuantityInput: (value: string) => void;
  onAdd: () => void;
  onUpdate: (index: number, patch: Partial<KeywordRule>) => void;
  onCommit: () => void;
  onToggle: (index: number, enabled: boolean) => void;
  onRemove: (index: number) => void;
  onRunKey: (keyword: string) => void;
  onSendOrder: (keyword: string) => void;
  busy: boolean;
}) {
  return (
    <Stack spacing={1.5}>
      <Typography variant="h6">Keylar ro'yxati</Typography>
      <Box
        sx={{
          display: 'grid',
          gridTemplateColumns: { xs: '1fr', sm: '1fr 1fr', md: '1.5fr 150px 150px auto' },
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
          label="Key (link)"
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
        <TextField
          label="Quality"
          value={quantityInput}
          onChange={(event) => setQuantityInput(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === 'Enter') onAdd();
          }}
          type="number"
          slotProps={{ htmlInput: { min: 1, max: 1000000 } }}
          fullWidth
        />
        <Button variant="outlined" onClick={onAdd} startIcon={<Plus size={18} />} sx={{ minWidth: 118 }}>
          Qo'shish
        </Button>
      </Box>

      {isMobile ? (
        <Stack spacing={1.25}>
          {rules.map((rule, index) => (
            <RuleCard
              key={`${rule.text}-${index}`}
              rule={rule}
              index={index}
              onUpdate={onUpdate}
              onCommit={onCommit}
              onToggle={onToggle}
              onRemove={onRemove}
              onRunKey={onRunKey}
              onSendOrder={onSendOrder}
              busy={busy}
            />
          ))}
          {!rules.length && <EmptyHint text="Bo'sh" />}
        </Stack>
      ) : (
        <Box sx={{ overflowX: 'auto' }}>
          <Table size="small" sx={{ minWidth: 980 }}>
            <TableHead>
              <TableRow>
                <TableCell sx={{ width: 88 }}>Holat</TableCell>
                <TableCell>Key</TableCell>
                <TableCell sx={{ width: 150 }}>Kutish</TableCell>
                <TableCell sx={{ width: 130 }}>Quality</TableCell>
                <TableCell sx={{ width: 160 }}>Oxirgi</TableCell>
                <TableCell sx={{ width: 160 }}>Keyingi</TableCell>
                <TableCell align="right" sx={{ width: 150 }} />
              </TableRow>
            </TableHead>
            <TableBody>
              {rules.map((rule, index) => (
                <TableRow key={`${rule.text}-${index}`} hover>
                  <TableCell>
                    <Switch
                      checked={rule.enabled}
                      onChange={(event) => onToggle(index, event.target.checked)}
                    />
                  </TableCell>
                  <TableCell>
                    <TextField
                      value={rule.text}
                      onChange={(event) => onUpdate(index, { text: event.target.value })}
                      onBlur={() => onCommit()}
                      size="small"
                      fullWidth
                    />
                  </TableCell>
                  <TableCell>
                    <TextField
                      value={rule.interval_seconds}
                      onChange={(event) => onUpdate(index, { interval_seconds: Number(event.target.value) })}
                      onBlur={() => onCommit()}
                      type="number"
                      size="small"
                      slotProps={{
                        htmlInput: { min: 2, max: 86400 },
                        input: { endAdornment: <InputAdornment position="end">s</InputAdornment> }
                      }}
                      fullWidth
                    />
                  </TableCell>
                  <TableCell>
                    <TextField
                      value={rule.order_quantity}
                      onChange={(event) => onUpdate(index, { order_quantity: Number(event.target.value) })}
                      onBlur={() => onCommit()}
                      type="number"
                      size="small"
                      slotProps={{ htmlInput: { min: 1, max: 1000000 } }}
                      fullWidth
                    />
                  </TableCell>
                  <TableCell sx={{ whiteSpace: 'nowrap' }}>{formatDate(rule.last_checked_at)}</TableCell>
                  <TableCell sx={{ whiteSpace: 'nowrap' }}>{formatDate(rule.next_check_at)}</TableCell>
                  <TableCell align="right" sx={{ whiteSpace: 'nowrap' }}>
                    <Tooltip title={`1 ta buyurtma yuborish (quality ${rule.order_quantity})`}>
                      <span>
                        <IconButton
                          color="primary"
                          onClick={() => onSendOrder(rule.text)}
                          disabled={busy || !rule.text.trim()}
                        >
                          <Send size={18} />
                        </IconButton>
                      </span>
                    </Tooltip>
                    <Tooltip title="Hozir tekshirish">
                      <span>
                        <IconButton
                          color="secondary"
                          onClick={() => onRunKey(rule.text)}
                          disabled={busy || !rule.text.trim()}
                        >
                          <Play size={18} />
                        </IconButton>
                      </span>
                    </Tooltip>
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
                  <TableCell colSpan={7}>
                    <EmptyHint text="Bo'sh" />
                  </TableCell>
                </TableRow>
              )}
            </TableBody>
          </Table>
        </Box>
      )}
    </Stack>
  );
}

function RuleCard({
  rule,
  index,
  onUpdate,
  onCommit,
  onToggle,
  onRemove,
  onRunKey,
  onSendOrder,
  busy
}: {
  rule: KeywordRule;
  index: number;
  onUpdate: (index: number, patch: Partial<KeywordRule>) => void;
  onCommit: () => void;
  onToggle: (index: number, enabled: boolean) => void;
  onRemove: (index: number) => void;
  onRunKey: (keyword: string) => void;
  onSendOrder: (keyword: string) => void;
  busy: boolean;
}) {
  return (
    <Paper variant="outlined" sx={{ p: 1.5 }}>
      <Stack spacing={1.25}>
        <Stack direction="row" sx={{ justifyContent: 'space-between', alignItems: 'center' }}>
          <FormControlLabel
            control={
              <Switch checked={rule.enabled} onChange={(event) => onToggle(index, event.target.checked)} />
            }
            label={rule.enabled ? 'Avto yoqilgan' : "Avto o'chiq"}
          />
          <Tooltip title="O'chirish">
            <IconButton color="error" onClick={() => onRemove(index)}>
              <Trash2 size={18} />
            </IconButton>
          </Tooltip>
        </Stack>
        <TextField
          label="Key (link)"
          value={rule.text}
          onChange={(event) => onUpdate(index, { text: event.target.value })}
          onBlur={() => onCommit()}
          fullWidth
        />
        <Box sx={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 1 }}>
          <TextField
            label="Kutish"
            value={rule.interval_seconds}
            onChange={(event) => onUpdate(index, { interval_seconds: Number(event.target.value) })}
            onBlur={() => onCommit()}
            type="number"
            slotProps={{
              htmlInput: { min: 2, max: 86400 },
              input: { endAdornment: <InputAdornment position="end">s</InputAdornment> }
            }}
          />
          <TextField
            label="Quality"
            value={rule.order_quantity}
            onChange={(event) => onUpdate(index, { order_quantity: Number(event.target.value) })}
            onBlur={() => onCommit()}
            type="number"
            slotProps={{ htmlInput: { min: 1, max: 1000000 } }}
          />
        </Box>
        <Stack direction="row" sx={{ justifyContent: 'space-between' }}>
          <Typography variant="caption" color="text.secondary">
            Oxirgi: {formatDate(rule.last_checked_at)}
          </Typography>
          <Typography variant="caption" color="text.secondary">
            Keyingi: {formatDate(rule.next_check_at)}
          </Typography>
        </Stack>
        <Stack direction="row" spacing={1}>
          <Button
            variant="contained"
            color="primary"
            onClick={() => onSendOrder(rule.text)}
            disabled={busy || !rule.text.trim()}
            startIcon={<Send size={16} />}
            sx={{ flex: 1, minHeight: 44 }}
          >
            1 ta buyurtma
          </Button>
          <Button
            variant="outlined"
            color="secondary"
            onClick={() => onRunKey(rule.text)}
            disabled={busy || !rule.text.trim()}
            startIcon={<Play size={16} />}
            sx={{ flex: 1, minHeight: 44 }}
          >
            Tekshirish
          </Button>
        </Stack>
      </Stack>
    </Paper>
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
          <Chip key={item} label={item} onDelete={() => onRemove(item)} sx={{ maxWidth: '100%' }} />
        ))}
        {!items.length && <Typography color="text.secondary">Bo'sh</Typography>}
      </Stack>
    </Stack>
  );
}

type AccountsPanelProps = {
  token: string | null;
  accounts: AccountStatus[];
  apiConfigured: boolean;
  savedApiId: number | null;
  onRefresh: () => void;
  onNotice: (message: string) => void;
  onError: (message: string) => void;
};

function AccountsPanel({
  token,
  accounts,
  apiConfigured,
  savedApiId,
  onRefresh,
  onNotice,
  onError
}: AccountsPanelProps) {
  const [apiIdInput, setApiIdInput] = useState(savedApiId ? String(savedApiId) : '');
  const [apiHashInput, setApiHashInput] = useState('');
  const [savingCreds, setSavingCreds] = useState(false);

  const [qrActive, setQrActive] = useState(false);
  const [qrAccountId, setQrAccountId] = useState<string | null>(null);
  const [qrUrl, setQrUrl] = useState<string | null>(null);
  const [qrImage, setQrImage] = useState<string | null>(null);
  const [qrStatus, setQrStatus] = useState<'waiting' | 'password'>('waiting');
  const [qrPassword, setQrPassword] = useState('');
  const [qrBusy, setQrBusy] = useState(false);

  const closeQr = () => {
    setQrActive(false);
    setQrAccountId(null);
    setQrUrl(null);
    setQrImage(null);
    setQrStatus('waiting');
    setQrPassword('');
  };

  const saveCredentials = async () => {
    setSavingCreds(true);
    try {
      await apiFetch('/telegram/credentials', token, {
        method: 'POST',
        body: JSON.stringify({ api_id: Number(apiIdInput), api_hash: apiHashInput.trim() })
      });
      setApiHashInput('');
      onNotice("API ma'lumotlari saqlandi");
      onRefresh();
    } catch (err) {
      onError(err instanceof Error ? err.message : 'Saqlash xato');
    } finally {
      setSavingCreds(false);
    }
  };

  const startQr = async () => {
    setQrBusy(true);
    try {
      const data = await apiFetch<QrStartResponse>('/telegram/qr/start', token, { method: 'POST' });
      setQrAccountId(data.account_id);
      setQrUrl(data.qr_url);
      setQrStatus('waiting');
      setQrPassword('');
      setQrActive(true);
    } catch (err) {
      onError(err instanceof Error ? err.message : 'QR boshlashda xato');
    } finally {
      setQrBusy(false);
    }
  };

  const submitPassword = async () => {
    if (!qrAccountId) return;
    setQrBusy(true);
    try {
      const data = await apiFetch<QrPollResponse>('/telegram/qr/password', token, {
        method: 'POST',
        body: JSON.stringify({ account_id: qrAccountId, password: qrPassword })
      });
      if (data.status === 'connected') {
        closeQr();
        onNotice('Akkaunt ulandi');
        onRefresh();
      }
    } catch (err) {
      onError(err instanceof Error ? err.message : "2FA parol noto'g'ri");
    } finally {
      setQrBusy(false);
    }
  };

  const removeAccount = async (id: string) => {
    try {
      await apiFetch('/telegram/account/remove', token, {
        method: 'POST',
        body: JSON.stringify({ account_id: id })
      });
      onNotice("Akkaunt o'chirildi");
      onRefresh();
    } catch (err) {
      onError(err instanceof Error ? err.message : "O'chirish xato");
    }
  };

  // qr_url o'zgarganda QR rasmni chizamiz.
  useEffect(() => {
    if (!qrUrl) {
      setQrImage(null);
      return;
    }
    let active = true;
    QRCodeLib.toDataURL(qrUrl, { width: 240, margin: 1 })
      .then((url) => {
        if (active) setQrImage(url);
      })
      .catch(() => {
        if (active) setQrImage(null);
      });
    return () => {
      active = false;
    };
  }, [qrUrl]);

  // QR holatini har 2.5s da tekshiramiz (faqat skan kutilayotganda).
  useEffect(() => {
    if (!qrActive || !qrAccountId || qrStatus !== 'waiting') return;
    let cancelled = false;
    const id = window.setInterval(async () => {
      try {
        const data = await apiFetch<QrPollResponse>('/telegram/qr/poll', token, {
          method: 'POST',
          body: JSON.stringify({ account_id: qrAccountId })
        });
        if (cancelled) return;
        if (data.status === 'connected') {
          closeQr();
          onNotice('Akkaunt ulandi');
          onRefresh();
        } else if (data.status === 'password') {
          setQrStatus('password');
        } else if (data.status === 'waiting' && data.qr_url) {
          setQrUrl(data.qr_url);
        }
      } catch (err) {
        if (cancelled) return;
        closeQr();
        onError(err instanceof Error ? err.message : 'QR tekshirishda xato');
      }
    }, 2500);
    return () => {
      cancelled = true;
      window.clearInterval(id);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [qrActive, qrAccountId, qrStatus, token]);

  return (
    <Stack spacing={3}>
      {!apiConfigured ? (
        <Paper variant="outlined" sx={{ p: 2 }}>
          <Stack spacing={1.5}>
            <Typography variant="h6">Telegram API</Typography>
            <Typography variant="body2" color="text.secondary">
              QR orqali akkaunt qo'shishdan oldin API ID va API hash kiriting (my.telegram.org dan olinadi).
            </Typography>
            <Box
              sx={{
                display: 'grid',
                gridTemplateColumns: { xs: '1fr', sm: '1fr 1fr auto' },
                gap: 1.5
              }}
            >
              <TextField
                label="API ID"
                type="number"
                value={apiIdInput}
                onChange={(event) => setApiIdInput(event.target.value)}
                fullWidth
              />
              <TextField
                label="API hash"
                type="password"
                value={apiHashInput}
                onChange={(event) => setApiHashInput(event.target.value)}
                fullWidth
              />
              <Button
                variant="contained"
                onClick={saveCredentials}
                disabled={savingCreds}
                startIcon={<Save size={18} />}
                sx={{ minWidth: 120 }}
              >
                Saqlash
              </Button>
            </Box>
          </Stack>
        </Paper>
      ) : (
        <Stack direction="row" spacing={1} sx={{ alignItems: 'center', flexWrap: 'wrap' }}>
          <Chip
            color="secondary"
            icon={<CircleCheck size={16} />}
            label={`API sozlangan (ID: ${savedApiId})`}
            sx={{ fontWeight: 800 }}
          />
        </Stack>
      )}

      <Box>
        <Button
          variant="contained"
          onClick={startQr}
          disabled={!apiConfigured || qrBusy || qrActive}
          startIcon={<QrCode size={18} />}
          sx={{ width: { xs: '100%', sm: 'auto' } }}
        >
          QR bilan akkaunt qo'shish
        </Button>
      </Box>

      {qrActive && (
        <Paper variant="outlined" sx={{ p: { xs: 2, md: 3 } }}>
          <Stack spacing={2} sx={{ alignItems: 'center', textAlign: 'center' }}>
            {qrStatus === 'waiting' ? (
              <>
                <Typography variant="h6">QR kodni skanерlang</Typography>
                <Typography variant="body2" color="text.secondary" sx={{ maxWidth: 440 }}>
                  Telegram ilovasi → Sozlamalar → Qurilmalar → "Kompyuter qurilmasini ulash" →
                  shu QR kodni skanерlang.
                </Typography>
                {qrImage ? (
                  <Box
                    component="img"
                    src={qrImage}
                    alt="QR"
                    sx={{ width: 240, height: 240, borderRadius: 2, bgcolor: '#fff', p: 1 }}
                  />
                ) : (
                  <CircularProgress />
                )}
                <Stack direction="row" spacing={1} sx={{ alignItems: 'center' }}>
                  <CircularProgress size={16} />
                  <Typography variant="caption" color="text.secondary">
                    Ulanish kutilmoqda…
                  </Typography>
                </Stack>
              </>
            ) : (
              <>
                <Typography variant="h6">2FA parol</Typography>
                <Typography variant="body2" color="text.secondary" sx={{ maxWidth: 440 }}>
                  Bu akkauntda ikki bosqichli (2FA) parol yoqilgan. Parolni kiriting.
                </Typography>
                <TextField
                  label="2FA parol"
                  type="password"
                  value={qrPassword}
                  onChange={(event) => setQrPassword(event.target.value)}
                  onKeyDown={(event) => {
                    if (event.key === 'Enter') submitPassword();
                  }}
                  sx={{ maxWidth: 320, width: '100%' }}
                />
                <Button
                  variant="contained"
                  color="secondary"
                  onClick={submitPassword}
                  disabled={qrBusy}
                  startIcon={<CircleCheck size={18} />}
                >
                  Tasdiqlash
                </Button>
              </>
            )}
            <Button variant="text" onClick={closeQr}>
              Bekor qilish
            </Button>
          </Stack>
        </Paper>
      )}

      <Stack spacing={1.5}>
        <Typography variant="h6">Akkauntlar ({accounts.length})</Typography>
        {accounts.length === 0 && <EmptyHint text="Akkaunt yo'q — QR bilan qo'shing" />}
        <Stack spacing={1}>
          {accounts.map((account, index) => {
            const fullName = [account.first_name, account.last_name].filter(Boolean).join(' ');
            const title = fullName || account.label || account.username || account.id.slice(0, 8);
            return (
              <Paper key={account.id} variant="outlined" sx={{ p: 1.5 }}>
                <Stack
                  direction="row"
                  sx={{ justifyContent: 'space-between', alignItems: 'flex-start', gap: 1 }}
                >
                  <Stack direction="row" spacing={1.25} sx={{ minWidth: 0, alignItems: 'flex-start' }}>
                    <Chip
                      size="small"
                      color="primary"
                      label={`#${index + 1}`}
                      sx={{ fontWeight: 800, mt: 0.25 }}
                    />
                    <Box sx={{ minWidth: 0 }}>
                      <Typography sx={{ fontWeight: 800 }} className="text-clamp">
                        {title}
                      </Typography>
                      <Stack
                        direction="row"
                        spacing={1.5}
                        sx={{ mt: 0.25, flexWrap: 'wrap', rowGap: 0.25 }}
                      >
                        {account.username && (
                          <Typography variant="body2" color="text.secondary" noWrap>
                            @{account.username}
                          </Typography>
                        )}
                        {account.phone && (
                          <Typography variant="body2" color="text.secondary" noWrap>
                            📞 +{account.phone.replace(/^\+/, '')}
                          </Typography>
                        )}
                        {account.telegram_id != null && (
                          <Typography variant="body2" color="text.secondary" noWrap>
                            ID: {account.telegram_id}
                          </Typography>
                        )}
                      </Stack>
                      <Stack direction="row" spacing={0.75} sx={{ mt: 0.5, flexWrap: 'wrap', rowGap: 0.5 }}>
                        <Chip
                          size="small"
                          color={account.connected ? 'secondary' : 'default'}
                          icon={account.connected ? <CircleCheck size={14} /> : <CircleOff size={14} />}
                          label={account.connected ? 'Ulangan' : 'Ulanmagan'}
                        />
                        {account.flooded && (
                          <Chip size="small" color="warning" label={`Limit: ${formatDate(account.flood_until)}`} />
                        )}
                        <Chip
                          size="small"
                          variant="outlined"
                          label={`Qo'shilgan: ${formatDate(account.created_at)}`}
                        />
                        {account.last_used_at && (
                          <Chip
                            size="small"
                            variant="outlined"
                            label={`Oxirgi ishlatilgan: ${formatDate(account.last_used_at)}`}
                          />
                        )}
                      </Stack>
                    </Box>
                  </Stack>
                  <Tooltip title="O'chirish">
                    <IconButton color="error" onClick={() => removeAccount(account.id)}>
                      <Trash2 size={18} />
                    </IconButton>
                  </Tooltip>
                </Stack>
              </Paper>
            );
          })}
        </Stack>
      </Stack>
    </Stack>
  );
}

const DONUT_GREEN = ['#2e7d32', '#43a047', '#66bb6a', '#81c784', '#a5d6a7', '#c8e6c9'];
const DONUT_RED = ['#c62828', '#e53935', '#ef5350', '#ff7043', '#ff8a65', '#ffab91'];

function StatsDonuts({ stats }: { stats: KeywordStat[] }) {
  if (!stats.length) {
    return (
      <Paper variant="outlined" sx={{ p: 2 }}>
        <Typography variant="h6" sx={{ mb: 0.5 }}>
          So'nggi 24 soat — kalit so'zlar bo'yicha
        </Typography>
        <Typography variant="body2" color="text.secondary">
          Hali ma'lumot yo'q — skaner reklama topgach diagramma shakllanadi.
        </Typography>
      </Paper>
    );
  }

  return (
    <Paper variant="outlined" sx={{ p: { xs: 1.5, md: 2 } }}>
      <Stack direction="row" spacing={1} sx={{ alignItems: 'center', mb: 1.5, flexWrap: 'wrap' }}>
        <Typography variant="h6">So'nggi 24 soat — kalit so'zlar bo'yicha</Typography>
        <Chip
          size="small"
          label="🟢 oq ro'yxat"
          sx={{ bgcolor: (t) => alpha(t.palette.success.main, 0.16), fontWeight: 700 }}
        />
        <Chip
          size="small"
          label="🔴 order ketadi"
          sx={{ bgcolor: (t) => alpha(t.palette.error.main, 0.16), fontWeight: 700 }}
        />
      </Stack>
      <Box
        sx={{
          display: 'grid',
          gridTemplateColumns: { xs: '1fr', sm: '1fr 1fr', lg: 'repeat(3, 1fr)' },
          gap: { xs: 1.5, md: 2 }
        }}
      >
        {stats.map((stat) => (
          <KeywordDonut key={stat.keyword} stat={stat} />
        ))}
      </Box>
    </Paper>
  );
}

function KeywordDonut({ stat }: { stat: KeywordStat }) {
  const size = 150;
  const stroke = 22;
  const radius = (size - stroke) / 2;
  const circumference = 2 * Math.PI * radius;

  let offset = 0;
  let greenIdx = 0;
  let redIdx = 0;
  const arcs = stat.segments.map((seg) => {
    const len = (seg.percent / 100) * circumference;
    const color = seg.whitelisted
      ? DONUT_GREEN[greenIdx++ % DONUT_GREEN.length]
      : DONUT_RED[redIdx++ % DONUT_RED.length];
    const dash = `${len} ${circumference - len}`;
    const arc = { seg, color, dash, dashoffset: -offset };
    offset += len;
    return arc;
  });

  return (
    <Paper variant="outlined" sx={{ p: 1.5, borderRadius: 3 }}>
      <Stack direction="row" spacing={1.5} sx={{ alignItems: 'center' }}>
        <Box sx={{ position: 'relative', width: size, height: size, flexShrink: 0 }}>
          <svg width={size} height={size} viewBox={`0 0 ${size} ${size}`}>
            <g transform={`rotate(-90 ${size / 2} ${size / 2})`}>
              <circle
                cx={size / 2}
                cy={size / 2}
                r={radius}
                fill="none"
                stroke="#eee"
                strokeWidth={stroke}
              />
              {arcs.map((arc, i) => (
                <circle
                  key={i}
                  cx={size / 2}
                  cy={size / 2}
                  r={radius}
                  fill="none"
                  stroke={arc.color}
                  strokeWidth={stroke}
                  strokeDasharray={arc.dash}
                  strokeDashoffset={arc.dashoffset}
                  strokeLinecap="butt"
                >
                  <title>{`@${arc.seg.channel} — ${arc.seg.percent.toFixed(1)}% (${arc.seg.count})`}</title>
                </circle>
              ))}
            </g>
          </svg>
          <Box
            sx={{
              position: 'absolute',
              inset: 0,
              display: 'flex',
              flexDirection: 'column',
              alignItems: 'center',
              justifyContent: 'center',
              textAlign: 'center',
              px: 1
            }}
          >
            <Typography sx={{ fontWeight: 800, fontSize: '0.95rem', lineHeight: 1 }} className="text-clamp">
              {stat.keyword}
            </Typography>
            <Typography variant="caption" color="text.secondary">
              {stat.total} ta
            </Typography>
          </Box>
        </Box>
        <Stack spacing={0.5} sx={{ minWidth: 0, flex: 1 }}>
          {stat.segments.map((seg, i) => (
            <Stack key={seg.channel} direction="row" spacing={0.75} sx={{ alignItems: 'center', minWidth: 0 }}>
              <Box
                sx={{
                  width: 10,
                  height: 10,
                  borderRadius: '50%',
                  flexShrink: 0,
                  bgcolor: seg.whitelisted
                    ? DONUT_GREEN[stat.segments.slice(0, i).filter((s) => s.whitelisted).length % DONUT_GREEN.length]
                    : DONUT_RED[stat.segments.slice(0, i).filter((s) => !s.whitelisted).length % DONUT_RED.length]
                }}
              />
              <Typography variant="caption" sx={{ fontWeight: 700, flexShrink: 0 }}>
                {seg.percent.toFixed(0)}%
              </Typography>
              <Typography variant="caption" color="text.secondary" noWrap sx={{ minWidth: 0 }}>
                @{seg.channel}
              </Typography>
            </Stack>
          ))}
        </Stack>
      </Stack>
    </Paper>
  );
}

function ResultsPanel({
  results,
  whitelist,
  stats,
  runScan,
  clearResults,
  busy,
  isMobile
}: {
  results: AdResult[];
  whitelist: string[];
  stats: KeywordStat[];
  runScan: () => void;
  clearResults: () => void;
  busy: boolean;
  isMobile: boolean;
}) {
  const sorted = useMemo(() => results, [results]);

  return (
    <Stack spacing={2}>
      <StatsDonuts stats={stats} />

      <Stack
        direction={{ xs: 'column', sm: 'row' }}
        spacing={1}
        sx={{ justifyContent: 'space-between', alignItems: { sm: 'center' } }}
      >
        <Stack spacing={0.75}>
          <Typography variant="h6">Topilgan ads ({sorted.length})</Typography>
          <Stack direction="row" spacing={1} sx={{ flexWrap: 'wrap' }}>
            <Chip
              size="small"
              label="Oq ro'yxat → order yo'q"
              sx={{ bgcolor: (theme) => alpha(theme.palette.success.main, 0.18), fontWeight: 700 }}
            />
            <Chip
              size="small"
              label="Qolganlar → order"
              sx={{ bgcolor: (theme) => alpha(theme.palette.error.main, 0.16), fontWeight: 700 }}
            />
          </Stack>
        </Stack>
        <Stack direction="row" spacing={1}>
          <Button variant="contained" onClick={runScan} disabled={busy} startIcon={<Play size={18} />} sx={{ flex: { xs: 1, sm: 'initial' } }}>
            Hozir tekshirish
          </Button>
          <Button
            variant="outlined"
            color="error"
            onClick={clearResults}
            disabled={busy}
            startIcon={<Trash2 size={18} />}
            sx={{ flex: { xs: 1, sm: 'initial' } }}
          >
            Tozalash
          </Button>
        </Stack>
      </Stack>

      {isMobile ? (
        <Stack spacing={1.25}>
          {sorted.map((item) => (
            <ResultCard key={item.id} item={item} tone={classifyResult(item, whitelist)} />
          ))}
          {!sorted.length && <EmptyHint text="Natija yo'q" />}
        </Stack>
      ) : (
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
                <TableRow key={item.id} hover sx={rowToneSx(classifyResult(item, whitelist))}>
                  <TableCell sx={{ whiteSpace: 'nowrap' }}>{formatDate(item.found_at)}</TableCell>
                  <TableCell className="text-clamp">
                    <Typography sx={{ fontWeight: 800 }}>{item.channel_title || item.channel}</Typography>
                    <Typography variant="caption" color="text.secondary">
                      @{item.channel}
                    </Typography>
                  </TableCell>
                  <TableCell className="text-clamp">{formatChannel(item.target_channel)}</TableCell>
                  <TableCell>
                    <Stack direction="row" sx={{ gap: 0.5, flexWrap: 'wrap' }}>
                      {(item.matched_keywords.length ? item.matched_keywords : ['all']).map((keyword) => (
                        <Chip key={keyword} label={keyword} size="small" color="secondary" />
                      ))}
                    </Stack>
                  </TableCell>
                  <TableCell className="text-clamp" sx={{ maxWidth: 220 }}>
                    {item.title}
                  </TableCell>
                  <TableCell className="text-clamp" sx={{ maxWidth: 340 }}>
                    {item.message}
                  </TableCell>
                  <TableCell className="text-clamp" sx={{ maxWidth: 240 }}>
                    {item.sponsor_info || '-'}
                  </TableCell>
                  <TableCell className="text-clamp" sx={{ maxWidth: 240 }}>
                    {item.additional_info || '-'}
                  </TableCell>
                  <TableCell className="text-clamp" sx={{ maxWidth: 220 }}>
                    <Link href={item.url} target="_blank" rel="noreferrer">
                      {item.url}
                    </Link>
                    <Typography variant="caption" sx={{ display: 'block' }} color="text.secondary">
                      {item.button_text}
                    </Typography>
                    {item.recommended && <Chip label="recommended" size="small" color="secondary" sx={{ mt: 0.75 }} />}
                  </TableCell>
                  <TableCell className="text-clamp" sx={{ maxWidth: 180 }}>
                    <Typography variant="caption">{item.random_id_hex}</Typography>
                  </TableCell>
                </TableRow>
              ))}
              {!sorted.length && (
                <TableRow>
                  <TableCell colSpan={10}>
                    <EmptyHint text="Natija yo'q" />
                  </TableCell>
                </TableRow>
              )}
            </TableBody>
          </Table>
        </Box>
      )}
    </Stack>
  );
}

function ResultCard({ item, tone }: { item: AdResult; tone: Tone }) {
  return (
    <Paper
      variant="outlined"
      sx={(theme) => ({
        p: 1.5,
        ...(tone === 'white' && {
          borderLeft: `4px solid ${theme.palette.success.main}`,
          bgcolor: alpha(theme.palette.success.main, 0.1)
        }),
        ...(tone === 'black' && {
          borderLeft: `4px solid ${theme.palette.error.main}`,
          bgcolor: alpha(theme.palette.error.main, 0.09)
        })
      })}
    >
      <Stack spacing={0.75}>
        <Stack direction="row" sx={{ justifyContent: 'space-between', alignItems: 'flex-start', gap: 1 }}>
          <Box sx={{ minWidth: 0 }}>
            <Typography sx={{ fontWeight: 800 }} className="text-clamp">
              {item.channel_title || item.channel}
            </Typography>
            <Typography variant="caption" color="text.secondary">
              @{item.channel}
            </Typography>
          </Box>
          <Typography variant="caption" color="text.secondary" sx={{ whiteSpace: 'nowrap' }}>
            {formatDate(item.found_at)}
          </Typography>
        </Stack>

        <Stack direction="row" sx={{ gap: 0.5, flexWrap: 'wrap' }}>
          {(item.matched_keywords.length ? item.matched_keywords : ['all']).map((keyword) => (
            <Chip key={keyword} label={keyword} size="small" color="secondary" />
          ))}
          {item.recommended && <Chip label="recommended" size="small" />}
        </Stack>

        {item.title && (
          <Typography sx={{ fontWeight: 700 }} className="text-clamp">
            {item.title}
          </Typography>
        )}
        {item.message && (
          <Typography variant="body2" className="text-clamp" color="text.secondary">
            {item.message}
          </Typography>
        )}

        <Divider sx={{ my: 0.5 }} />
        <FieldRow label="Target">{formatChannel(item.target_channel)}</FieldRow>
        {item.sponsor_info && (
          <FieldRow label="Sponsor">
            <span className="text-clamp">{item.sponsor_info}</span>
          </FieldRow>
        )}
        <FieldRow label="URL">
          <Link href={item.url} target="_blank" rel="noreferrer" className="text-clamp">
            {item.button_text || item.url}
          </Link>
        </FieldRow>
      </Stack>
    </Paper>
  );
}

function LogsPanel({
  logs,
  clearLogs,
  busy,
  isMobile
}: {
  logs: PanelLog[];
  clearLogs: () => void;
  busy: boolean;
  isMobile: boolean;
}) {
  return (
    <Stack spacing={2}>
      <Stack
        direction={{ xs: 'column', sm: 'row' }}
        spacing={1}
        sx={{ justifyContent: 'space-between', alignItems: { sm: 'center' } }}
      >
        <Box>
          <Typography variant="h6">Loglar ({logs.length})</Typography>
          <Typography variant="body2" color="text.secondary">
            Skan, oq ro'yxat qarori va BAZA order javoblari
          </Typography>
        </Box>
        <Button
          variant="outlined"
          color="error"
          onClick={clearLogs}
          disabled={busy}
          startIcon={<Trash2 size={18} />}
          sx={{ flex: { xs: 1, sm: 'initial' } }}
        >
          Tozalash
        </Button>
      </Stack>

      {isMobile ? (
        <Stack spacing={1.25}>
          {logs.map((log) => (
            <LogCard key={log.id} log={log} />
          ))}
          {!logs.length && <EmptyHint text="Log yo'q" />}
        </Stack>
      ) : (
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
                <TableCell>Sabab / Natija</TableCell>
              </TableRow>
            </TableHead>
            <TableBody>
              {logs.map((log) => {
                const hasOrder = Boolean(log.quantity || log.order_id);
                return (
                  <TableRow key={log.id} hover>
                    <TableCell sx={{ whiteSpace: 'nowrap' }}>{formatDate(log.created_at)}</TableCell>
                    <TableCell>
                      <LevelChip level={log.level} />
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
                    <TableCell className="text-clamp">{log.keyword || '—'}</TableCell>
                    <TableCell className="text-clamp">{log.source_channel || '—'}</TableCell>
                    <TableCell className="text-clamp">{log.target_channel || '—'}</TableCell>
                    <TableCell className="text-clamp" sx={{ maxWidth: 240 }}>
                      {hasOrder ? (
                        <>
                          <Typography variant="body2">soni: {log.quantity ?? '-'}</Typography>
                          <Typography variant="body2">order: {log.order_id ?? '-'}</Typography>
                          {log.order_link && (
                            <Link href={log.order_link} target="_blank" rel="noreferrer" variant="caption">
                              {log.order_link}
                            </Link>
                          )}
                        </>
                      ) : (
                        <Typography variant="body2" color="text.secondary">
                          order yo'q
                        </Typography>
                      )}
                    </TableCell>
                    <TableCell className="text-clamp" sx={{ maxWidth: 340, whiteSpace: 'pre-line' }}>
                      {log.raw_response || '—'}
                    </TableCell>
                  </TableRow>
                );
              })}
              {!logs.length && (
                <TableRow>
                  <TableCell colSpan={8}>
                    <EmptyHint text="Log yo'q" />
                  </TableCell>
                </TableRow>
              )}
            </TableBody>
          </Table>
        </Box>
      )}
    </Stack>
  );
}

function LogCard({ log }: { log: PanelLog }) {
  return (
    <Paper variant="outlined" sx={{ p: 1.5 }}>
      <Stack spacing={0.75}>
        <Stack direction="row" sx={{ justifyContent: 'space-between', alignItems: 'center', gap: 1 }}>
          <LevelChip level={log.level} />
          <Typography variant="caption" color="text.secondary" sx={{ whiteSpace: 'nowrap' }}>
            {formatDate(log.created_at)}
          </Typography>
        </Stack>
        <Typography sx={{ fontWeight: 800 }} className="text-clamp">
          {log.title}
        </Typography>
        <Typography variant="body2" className="text-clamp">
          {log.message}
        </Typography>
        {log.keyword && <FieldRow label="Key">{log.keyword}</FieldRow>}
        {log.source_channel && <FieldRow label="Source">{log.source_channel}</FieldRow>}
        {log.target_channel && <FieldRow label="Target">{log.target_channel}</FieldRow>}
        {(log.quantity || log.order_id) && (
          <FieldRow label="Order">
            <span>{`q:${log.quantity ?? '-'} · #${log.order_id ?? '-'}`}</span>
          </FieldRow>
        )}
        {log.order_link && (
          <FieldRow label="Link">
            <Link href={log.order_link} target="_blank" rel="noreferrer" className="text-clamp">
              {log.order_link}
            </Link>
          </FieldRow>
        )}
        {log.raw_response && (
          <Typography
            variant="caption"
            color="text.secondary"
            className="text-clamp"
            sx={{ display: 'block', mt: 0.5, whiteSpace: 'pre-line' }}
          >
            {log.raw_response}
          </Typography>
        )}
      </Stack>
    </Paper>
  );
}

function LevelChip({ level }: { level: string }) {
  return (
    <Chip
      size="small"
      label={level}
      color={level === 'success' ? 'secondary' : level === 'error' ? 'error' : level === 'warning' ? 'warning' : 'default'}
    />
  );
}

function FieldRow({ label, children }: { label: string; children: ReactNode }) {
  return (
    <Box sx={{ display: 'flex', justifyContent: 'space-between', gap: 1, alignItems: 'baseline' }}>
      <Typography variant="caption" color="text.secondary" sx={{ fontWeight: 700, whiteSpace: 'nowrap' }}>
        {label}
      </Typography>
      <Box sx={{ minWidth: 0, textAlign: 'right' }}>
        {typeof children === 'string' ? (
          <Typography variant="body2" className="text-clamp">
            {children}
          </Typography>
        ) : (
          children
        )}
      </Box>
    </Box>
  );
}

function EmptyHint({ text }: { text: string }) {
  return (
    <Box
      sx={{
        p: 3,
        textAlign: 'center',
        borderRadius: 1,
        bgcolor: (theme) => alpha(theme.palette.primary.main, 0.04)
      }}
    >
      <Typography color="text.secondary">{text}</Typography>
    </Box>
  );
}

type Tone = 'white' | 'black' | null;

// Backenddagi normalize_channel_ref bilan bir xil: kanal username'ini ajratib oladi.
function normalizeChannelRef(raw?: string | null): string | null {
  if (!raw) return null;
  let value = raw.trim().replace(/^@+/, '').trim();
  for (const prefix of [
    'https://t.me/',
    'http://t.me/',
    't.me/',
    'https://telegram.me/',
    'telegram.me/'
  ]) {
    if (value.startsWith(prefix)) value = value.slice(prefix.length);
  }
  value = value.split(/[?/#]/)[0].replace(/^@+/, '').trim();
  return value ? value.toLowerCase() : null;
}

// Natija oq ro'yxatda bo'lsa 'white' (order yo'q), aks holda 'black' (order yuboriladi).
function classifyResult(item: AdResult, whitelist: string[]): Tone {
  const candidates = new Set<string>();
  const target = normalizeChannelRef(item.target_channel);
  if (target) candidates.add(target);
  const url = normalizeChannelRef(item.url);
  if (url) candidates.add(url);

  const inList = (list: string[]) =>
    list.some((raw) => {
      const n = normalizeChannelRef(raw);
      return n != null && candidates.has(n);
    });

  if (inList(whitelist)) return 'white';
  return 'black';
}

function rowToneSx(tone: Tone) {
  if (tone === 'white') {
    return {
      bgcolor: (theme: Theme) => alpha(theme.palette.success.main, 0.16),
      '&:hover': {
        bgcolor: (theme: Theme) => alpha(theme.palette.success.main, 0.24)
      },
      '& td:first-of-type': {
        borderLeft: (theme: Theme) =>
          `4px solid ${theme.palette.success.main}`
      }
    };
  }
  if (tone === 'black') {
    return {
      bgcolor: (theme: Theme) => alpha(theme.palette.error.main, 0.14),
      '&:hover': {
        bgcolor: (theme: Theme) => alpha(theme.palette.error.main, 0.22)
      },
      '& td:first-of-type': {
        borderLeft: (theme: Theme) =>
          `4px solid ${theme.palette.error.main}`
      }
    };
  }
  return {};
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
    ? settings.keyword_rules.map((rule) => ({
        ...rule,
        order_quantity: rule.order_quantity || settings.order_quantity || 100
      }))
    : settings.keywords.map((keyword) => ({
        text: keyword,
        interval_seconds: settings.interval_seconds || 5,
        order_quantity: settings.order_quantity || 100,
        enabled: true,
        last_checked_at: null,
        next_check_at: null
      }));

  return {
    ...settings,
    keyword_rules: keywordRules,
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
