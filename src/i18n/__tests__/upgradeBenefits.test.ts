import { describe, expect, it } from 'vitest'
import de from '../locales/de.json'
import en from '../locales/en.json'
import es from '../locales/es.json'
import fr from '../locales/fr.json'
import itLocale from '../locales/it.json'
import ja from '../locales/ja.json'
import ko from '../locales/ko.json'
import pt from '../locales/pt.json'
import ru from '../locales/ru.json'
import zh from '../locales/zh.json'

const locales = { de, en, es, fr, it: itLocale, ja, ko, pt, ru, zh }

const requiredBenefitKeys = ['title', 'stt', 'llm', 'noApiKey', 'backupScenes'] as const

describe('localized upgrade benefit messages', () => {
  it('defines all benefit keys for every locale', () => {
    for (const [locale, messages] of Object.entries(locales)) {
      const benefits = (
        messages as {
          upgrade?: { benefits?: Record<(typeof requiredBenefitKeys)[number], string> }
        }
      ).upgrade?.benefits

      expect(benefits, `${locale}.upgrade.benefits`).toEqual(expect.any(Object))

      for (const key of requiredBenefitKeys) {
        const value = benefits?.[key]
        expect(value, `${locale}.upgrade.benefits.${key}`).toEqual(expect.any(String))
        expect(value?.trim(), `${locale}.upgrade.benefits.${key}`).not.toBe('')
      }
    }
  })
})
