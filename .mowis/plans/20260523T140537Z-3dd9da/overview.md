I'll create a plan for building a simple Hello World Express app. Here's what we need to set up:

<plan>
[[task]]
id = "t1"
title = "Initialize Node.js project and install Express"
description = "Create a package.json file and install the Express dependency. Run `npm init -y` to scaffold the project, then `npm install express` to add Express."
deps = []
model_tier = "fast"
tool_budget = 10
files_hint = ["package.json"]

[[task]]
id = "t2"
title = "Create Express Hello World server"
description = "Create an `index.js` (or `app.js`) file that imports Express, creates an app instance, defines a GET route on '/' that responds with 'Hello, World!', and listens on port 3000. Also add a 'start' script in package.json pointing to the entry file."
deps = ["t1"]
model_tier = "fast"
tool_budget = 10
files_hint = ["index.js", "package.json"]

[[task]]
id = "t3"
title = "Add .gitignore file"
description = "Create a .gitignore file with standard Node.js ignores (node_modules/, etc.) so the dependency folder isn't tracked."
deps = []
model_tier = "fast"
tool_budget = 5
files_hint = [".gitignore"]
</plan>

This plan will:
1. **Initialize the project** — set up `package.json` and install Express
2. **Create the server** — a minimal Express app with a "Hello, World!" response on `GET /`, listening on port 3000
3. **Add .gitignore** — to keep `node_modules/` out of version control

Once done, you'll be able to run `npm start` and visit `http://localhost:3000` to see "Hello, World!" 🚀