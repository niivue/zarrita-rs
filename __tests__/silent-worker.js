// A decode worker that never answers, so a job sent to it stays in flight.
import { parentPort } from 'node:worker_threads'

parentPort.on('message', () => {})
