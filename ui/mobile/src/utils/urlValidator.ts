const ALLOWED_PROTOCOLS = new Set(['http:', 'https:', 'ws:', 'wss:']);

export interface UrlValidationResult {
  valid: boolean;
  error?: string;
}

export function validateServerUrl(url: string): UrlValidationResult {
  if (typeof url !== 'string' || !url.trim()) {
    return { valid: false, error: 'URL boş olmayan bir metin olmalıdır.' };
  }

  try {
    const parsed = new URL(url);
    if (!ALLOWED_PROTOCOLS.has(parsed.protocol)) {
      return { valid: false, error: 'URL http, https, ws veya wss kullanmalıdır.' };
    }
    if (!parsed.host) {
      return { valid: false, error: 'URL bir sunucu adresi içermelidir.' };
    }
    return { valid: true };
  } catch {
    return { valid: false, error: 'Geçersiz URL biçimi.' };
  }
}
