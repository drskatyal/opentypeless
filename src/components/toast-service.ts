export type ToastType = 'success' | 'error' | 'info'

type ToastHandler = (text: string, type?: ToastType) => void

let addToast: ToastHandler = () => {}

export function registerToastHandler(handler: ToastHandler) {
  addToast = handler
  return () => {
    if (addToast === handler) addToast = () => {}
  }
}

export function toast(text: string, type: ToastType = 'info') {
  addToast(text, type)
}

toast.success = (text: string) => toast(text, 'success')
toast.error = (text: string) => toast(text, 'error')
