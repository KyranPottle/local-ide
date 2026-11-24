import { ROOT_DIRECTORY, WorkspaceFileSystem } from "./WorkspaceFileSystem"

const SCRATCH_DIRECTORY_NAME = "scratch"

const readmeText = `# Scratch workspace

This folder is in the private storage of the browser for this site.
The files stay on this computer, and no other program can read them.

Use it to try the editor without access to a local folder.
`

const exampleText = `interface Task {
  title: string
  minutes: number
  done: boolean
}

const tasks: Task[] = [
  { title: "Read the architecture notes", minutes: 20, done: true },
  { title: "Open a local folder", minutes: 5, done: false },
]

// Sum of the minutes of each task that is not done
export function remainingMinutes(list: Task[]): number {
  return list.filter((task) => !task.done).reduce((total, task) => total + task.minutes, 0)
}

console.log(\`Remaining: \${remainingMinutes(tasks)} minutes\`)
`

export async function openScratchDirectory(): Promise<FileSystemDirectoryHandle> {
  const privateRoot = await navigator.storage.getDirectory()
  const directory = await privateRoot.getDirectoryHandle(SCRATCH_DIRECTORY_NAME, { create: true })
  const fileSystem = new WorkspaceFileSystem(directory)
  const entries = await fileSystem.listDirectory(ROOT_DIRECTORY)
  if (entries.length === 0) {
    await fileSystem.writeTextFile("README.md", readmeText)
    await fileSystem.writeTextFile("src/example.ts", exampleText)
  }
  return directory
}
