import en from './en.json' with { type: 'json' };
import zh from './zh-CN.json' with { type: 'json' };
import pt from './pt.json' with { type: 'json' };
import es from './es.json' with { type: 'json' };
import ru from './ru.json' with { type: 'json' };

export const languages = { en, 'zh-cn': zh, pt, es, ru };
export const codes = Object.keys(languages);
export const dictionary = locale => languages[locale] ?? en;
export const label = key => ({ label: en[key], translations: Object.fromEntries(codes.filter(code => code !== 'en').map(code => [code, languages[code][key]])) });
