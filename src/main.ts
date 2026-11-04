import { mount } from 'svelte'
import App from './App.svelte'
import './app.css'

const target = document.getElementById('app')
if (!target) throw new Error('#app not found')

// Remove the static boot placeholder so it never lingers behind the app.
document.getElementById('boot')?.remove()

export default mount(App, { target })
