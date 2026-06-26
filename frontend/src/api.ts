export type KeywordRule = {
  text: string;
  interval_seconds: number;
  order_quantity: number;
  service_id: number;
  enabled: boolean;
  last_checked_at?: string | null;
  next_check_at?: string | null;
};

export type Settings = {
  enabled: boolean;
  interval_seconds: number;
  keywords: string[];
  keyword_rules: KeywordRule[];
  channels: string[];
  blacklist_channels: string[];
  whitelist_channels: string[];
  order_quantity: number;
  max_results: number;
};

export type TelegramSettings = {
  api_id?: number | null;
  api_hash?: string | null;
  phone?: string | null;
};

export type RuntimeStatus = {
  telegram_connected: boolean;
  login_waiting_for?: string | null;
  scanning: boolean;
  last_run_at?: string | null;
  next_run_at?: string | null;
  last_error?: string | null;
  total_results: number;
  total_logs: number;
};

export type AdResult = {
  id: string;
  fingerprint: string;
  channel: string;
  channel_title?: string | null;
  target_channel?: string | null;
  matched_keywords: string[];
  title: string;
  message: string;
  url: string;
  button_text: string;
  sponsor_info?: string | null;
  additional_info?: string | null;
  recommended: boolean;
  random_id_hex: string;
  found_at: string;
};

export type PanelLog = {
  id: string;
  created_at: string;
  level: string;
  title: string;
  message: string;
  keyword?: string | null;
  source_channel?: string | null;
  target_channel?: string | null;
  ad_url?: string | null;
  order_link?: string | null;
  quantity?: number | null;
  service_id?: number | null;
  order_id?: string | null;
  raw_response?: string | null;
};

export type SmmBalance = {
  configured: boolean;
  balance?: string | null;
  currency?: string | null;
  error?: string | null;
  checked_at: string;
};

export type AccountStatus = {
  id: string;
  label?: string | null;
  username?: string | null;
  connected: boolean;
  flooded: boolean;
  flood_until?: string | null;
  created_at: string;
  last_used_at?: string | null;
};

export type Dashboard = {
  settings: Settings;
  telegram: TelegramSettings;
  smm_balance: SmmBalance;
  status: RuntimeStatus;
  results: AdResult[];
  logs: PanelLog[];
  accounts: AccountStatus[];
};

export type LoginResponse = {
  token: string;
};

export type QrStartResponse = {
  account_id: string;
  qr_url: string;
  expires_at: string;
};

export type QrPollResponse = {
  account_id: string;
  status: 'waiting' | 'password' | 'connected' | 'error';
  qr_url?: string | null;
  expires_at?: string | null;
  message: string;
};

export type ScanResponse = {
  added: number;
  checked_channels: number;
  checked_keywords: number;
  message: string;
};

export class ApiError extends Error {
  status: number;
  constructor(message: string, status: number) {
    super(message);
    this.name = 'ApiError';
    this.status = status;
  }
}

export async function apiFetch<T>(
  path: string,
  token: string | null,
  init: RequestInit = {}
): Promise<T> {
  const headers = new Headers(init.headers);
  if (token) {
    headers.set('Authorization', `Bearer ${token}`);
  }
  if (init.body && !headers.has('Content-Type')) {
    headers.set('Content-Type', 'application/json');
  }

  let response: Response;
  try {
    response = await fetch(`/api${path}`, { ...init, headers });
  } catch {
    throw new ApiError("Serverga ulanib bo'lmadi", 0);
  }

  const raw = await response.text();
  let data: unknown = null;
  if (raw) {
    try {
      data = JSON.parse(raw);
    } catch {
      // JSON bo'lmagan javob (masalan proxy HTML xatosi) — null qoldiramiz.
      data = null;
    }
  }

  if (!response.ok) {
    const message =
      (data && typeof data === 'object' && 'error' in data
        ? String((data as { error: unknown }).error)
        : null) ?? `HTTP ${response.status}`;
    throw new ApiError(message, response.status);
  }

  return data as T;
}
