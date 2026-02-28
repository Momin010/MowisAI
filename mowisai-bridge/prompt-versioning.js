// Prompt Versioning System
// Version control for prompts and system messages

export class PromptVersionManager {
  constructor() {
    this.prompts = new Map();
    this.versions = new Map();
    this.currentVersions = new Map();
  }

  // Create a new prompt
  createPrompt(promptId, initialContent, metadata = {}) {
    if (this.prompts.has(promptId)) {
      throw new Error(`Prompt ${promptId} already exists`);
    }

    const prompt = {
      id: promptId,
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
      metadata,
      totalVersions: 0
    };

    this.prompts.set(promptId, prompt);

    // Create first version
    const version = this._createVersion(promptId, initialContent, 'Initial version', metadata.author || 'system');
    this.currentVersions.set(promptId, version.versionNumber);

    return prompt;
  }

  // Create a new version of a prompt
  createVersion(promptId, content, message = '', author = 'system') {
    const prompt = this.prompts.get(promptId);
    if (!prompt) throw new Error(`Prompt ${promptId} not found`);

    const version = this._createVersion(promptId, content, message, author);
    prompt.updatedAt = new Date().toISOString();
    prompt.totalVersions++;
    this.currentVersions.set(promptId, version.versionNumber);

    return version;
  }

  _createVersion(promptId, content, message, author) {
    const key = `${promptId}:versions`;
    let versionsList = this.versions.get(key) || [];

    const versionNumber = versionsList.length + 1;
    const version = {
      promptId,
      versionNumber,
      content,
      message,
      author,
      createdAt: new Date().toISOString(),
      hash: this._hashContent(content)
    };

    versionsList.push(version);
    this.versions.set(key, versionsList);

    return version;
  }

  _hashContent(content) {
    // Simple hash for content comparison
    let hash = 0;
    for (let i = 0; i < content.length; i++) {
      const char = content.charCodeAt(i);
      hash = ((hash << 5) - hash) + char;
      hash = hash & hash;
    }
    return hash.toString(16);
  }

  // Get current version of a prompt
  getCurrentVersion(promptId) {
    const versionNumber = this.currentVersions.get(promptId);
    if (versionNumber === undefined) {
      throw new Error(`Prompt ${promptId} not found`);
    }

    return this.getVersion(promptId, versionNumber);
  }

  // Get specific version
  getVersion(promptId, versionNumber) {
    const key = `${promptId}:versions`;
    const versions = this.versions.get(key) || [];

    const version = versions.find(v => v.versionNumber === versionNumber);
    if (!version) {
      throw new Error(`Version ${versionNumber} not found for prompt ${promptId}`);
    }

    return version;
  }

  // Get current prompt content
  getPromptContent(promptId) {
    const version = this.getCurrentVersion(promptId);
    return version.content;
  }

  // Get all versions of a prompt
  getAllVersions(promptId) {
    const key = `${promptId}:versions`;
    return this.versions.get(key) || [];
  }

  // Get version history
  getVersionHistory(promptId, limit = 10) {
    const versions = this.getAllVersions(promptId);
    return versions.slice(-limit).map(v => ({
      versionNumber: v.versionNumber,
      message: v.message,
      author: v.author,
      createdAt: v.createdAt,
      hash: v.hash
    }));
  }

  // Revert to previous version
  revertToVersion(promptId, versionNumber, author = 'system') {
    const version = this.getVersion(promptId, versionNumber);
    const newVersion = this.createVersion(
      promptId,
      version.content,
      `Reverted to version ${versionNumber}`,
      author
    );

    return newVersion;
  }

  // Compare two versions
  compareVersions(promptId, versionNumber1, versionNumber2) {
    const v1 = this.getVersion(promptId, versionNumber1);
    const v2 = this.getVersion(promptId, versionNumber2);

    const lines1 = v1.content.split('\n');
    const lines2 = v2.content.split('\n');

    const diff = {
      promptId,
      version1: versionNumber1,
      version2: versionNumber2,
      similar: v1.hash === v2.hash,
      content1: v1.content,
      content2: v2.content,
      lineCount1: lines1.length,
      lineCount2: lines2.length,
      changes: this._calculateDiff(lines1, lines2)
    };

    return diff;
  }

  _calculateDiff(lines1, lines2) {
    const changes = {
      added: 0,
      removed: 0,
      modified: 0
    };

    const maxLines = Math.max(lines1.length, lines2.length);
    for (let i = 0; i < maxLines; i++) {
      const line1 = lines1[i];
      const line2 = lines2[i];

      if (line1 === undefined) {
        changes.added++;
      } else if (line2 === undefined) {
        changes.removed++;
      } else if (line1 !== line2) {
        changes.modified++;
      }
    }

    return changes;
  }

  // Get all prompts
  getAllPrompts() {
    const prompts = [];
    for (const [id, prompt] of this.prompts.entries()) {
      prompts.push({
        ...prompt,
        currentVersion: this.currentVersions.get(id)
      });
    }
    return prompts;
  }

  // Search prompts
  searchPrompts(query) {
    const results = [];
    const queryLower = query.toLowerCase();

    for (const [id, prompt] of this.prompts.entries()) {
      if (id.toLowerCase().includes(queryLower)) {
        results.push({
          ...prompt,
          currentVersion: this.currentVersions.get(id)
        });
      }
    }

    return results;
  }

  // Delete prompt (including all versions)
  deletePrompt(promptId) {
    this.prompts.delete(promptId);
    this.versions.delete(`${promptId}:versions`);
    this.currentVersions.delete(promptId);
  }

  // Export prompt with all versions
  exportPrompt(promptId) {
    const prompt = this.prompts.get(promptId);
    if (!prompt) throw new Error(`Prompt ${promptId} not found`);

    return {
      prompt,
      versions: this.getAllVersions(promptId),
      currentVersion: this.currentVersions.get(promptId)
    };
  }

  // Import prompt from backup
  importPrompt(promptData) {
    const { prompt, versions } = promptData;

    this.prompts.set(prompt.id, prompt);

    for (const version of versions) {
      const key = `${prompt.id}:versions`;
      let versionsList = this.versions.get(key) || [];
      versionsList.push(version);
      this.versions.set(key, versionsList);
    }

    const currentVersion = promptData.currentVersion || 1;
    this.currentVersions.set(prompt.id, currentVersion);
  }

  // Get statistics
  getStats() {
    const stats = {
      totalPrompts: this.prompts.size,
      totalVersions: 0,
      averageVersionsPerPrompt: 0,
      prompts: []
    };

    for (const [id, prompt] of this.prompts.entries()) {
      const versions = this.getAllVersions(id);
      stats.totalVersions += versions.length;
      stats.prompts.push({
        id,
        versions: versions.length,
        createdAt: prompt.createdAt,
        updatedAt: prompt.updatedAt
      });
    }

    if (this.prompts.size > 0) {
      stats.averageVersionsPerPrompt = (stats.totalVersions / this.prompts.size).toFixed(2);
    }

    return stats;
  }

  // Clear all prompts
  clear() {
    this.prompts.clear();
    this.versions.clear();
    this.currentVersions.clear();
  }
}

// Export singleton instance
export const promptVersionManager = new PromptVersionManager();
