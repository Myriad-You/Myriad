import * as THREE from 'three'
import { GLTFLoader } from 'three/addons/loaders/GLTFLoader.js'

const root = typeof globalThis !== 'undefined' ? globalThis : window
root.THREE = THREE
root.GLTFLoader = GLTFLoader
