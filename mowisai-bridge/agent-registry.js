// Dynamic Agent Types Module
// Allows custom agent types to be loaded dynamically

export class AgentTypeRegistry {
  constructor() {
    this.agentTypes = new Map();
    this.customAgents = new Map();
    this._initializeDefaultAgents();
  }

  _initializeDefaultAgents() {
    const defaultAgents = {
      coder: {
        name: 'Coder',
        description: 'Specialized in software development and code implementation',
        systemPrompt: `You are a CODER agent specialized in software development.
Your role is to write code, create files, and implement technical solutions.
You have access to shell commands and file operations.
Focus on: correctness, best practices, testing, clean code.`,
        canCreateFiles: true,
        fileExtensions: ['.js', '.ts', '.py', '.go', '.rs', '.java', '.c', '.cpp', '.rb', '.php'],
        capabilities: ['shell_exec', 'file_read', 'file_write', 'file_list'],
        reviewCriteria: 'Check for syntax errors, logic bugs, missing tests, and code quality issues.'
      },
      researcher: {
        name: 'Researcher',
        description: 'Specialized in information gathering and analysis',
        systemPrompt: `You are a RESEARCHER agent specialized in information gathering and analysis.
Your role is to find, analyze, and synthesize information from various sources.
You perform literature reviews, competitive analysis, and fact-finding missions.`,
        canCreateFiles: true,
        fileExtensions: ['.md', '.txt', '.json', '.csv', '.pdf', '.docx', '.pptx'],
        capabilities: ['web_search', 'data_fetch', 'file_write', 'analysis'],
        reviewCriteria: 'Verify claims are supported, check reasoning is sound, identify gaps.'
      },
      designer: {
        name: 'Designer',
        description: 'Specialized in visual assets and document design',
        systemPrompt: `You are a DESIGNER agent specialized in creating visual assets and documents.
Your role is to generate images, layouts, presentations, PDFs, and design files.
You have access to imagemagick, pandoc, and file operations.`,
        canCreateFiles: true,
        fileExtensions: ['.svg', '.png', '.jpg', '.pdf', '.html', '.pptx', '.docx'],
        capabilities: ['image_create', 'pdf_generate', 'file_write', 'design'],
        reviewCriteria: 'Check output files are valid and visually professional.'
      },
      writer: {
        name: 'Writer',
        description: 'Specialized in content creation and documentation',
        systemPrompt: `You are a WRITER agent specialized in content creation.
Your role is to write documents, articles, copy, documentation, and creative content.
You craft clear, engaging, and appropriate content for the target audience.`,
        canCreateFiles: true,
        fileExtensions: ['.md', '.txt', '.docx', '.html', '.pdf', '.pptx'],
        capabilities: ['file_write', 'file_read', 'research'],
        reviewCriteria: 'Check grammar, clarity, structure, and audience appropriateness.'
      },
      financial_analyst: {
        name: 'Financial Analyst',
        description: 'Specialized in financial modeling and analysis',
        systemPrompt: `You are a FINANCIAL ANALYST agent specialized in financial modeling and analysis.
Your role is to analyze financial data, create spreadsheets, build financial models, and generate reports.
You can use calculators, create CSV/Excel files, and perform calculations.`,
        canCreateFiles: true,
        fileExtensions: ['.csv', '.xlsx', '.json', '.md', '.pdf', '.docx', '.pptx'],
        capabilities: ['calculation', 'data_analysis', 'file_write', 'chart_generation'],
        reviewCriteria: 'Verify calculations are correct, check assumptions are reasonable.'
      },
      data_scientist: {
        name: 'Data Scientist',
        description: 'Specialized in data analysis and machine learning',
        systemPrompt: `You are a DATA SCIENTIST agent specialized in data analysis and machine learning.
Your role is to analyze datasets, create visualizations, build models, and derive insights.
You can use Python with pandas, numpy, matplotlib, and other data tools.`,
        canCreateFiles: true,
        fileExtensions: ['.py', '.ipynb', '.json', '.csv', '.pdf', '.png', '.svg'],
        capabilities: ['data_analysis', 'modeling', 'visualization', 'file_write'],
        reviewCriteria: 'Verify methodology is sound, check model assumptions and results.'
      }
    };

    for (const [key, agentType] of Object.entries(defaultAgents)) {
      this.registerAgentType(key, agentType);
    }
  }

  // Register a new agent type
  registerAgentType(agentTypeId, agentTypeConfig) {
    const config = {
      id: agentTypeId,
      name: agentTypeConfig.name || agentTypeId,
      description: agentTypeConfig.description || '',
      systemPrompt: agentTypeConfig.systemPrompt || '',
      canCreateFiles: agentTypeConfig.canCreateFiles || false,
      fileExtensions: agentTypeConfig.fileExtensions || [],
      capabilities: agentTypeConfig.capabilities || [],
      reviewCriteria: agentTypeConfig.reviewCriteria || '',
      custom: true,
      registeredAt: new Date().toISOString()
    };

    this.agentTypes.set(agentTypeId, config);
    return config;
  }

  // Register a custom agent dynamically
  registerCustomAgent(agentId, agentClass, config = {}) {
    if (!agentClass) throw new Error('Agent class is required');

    const customConfig = {
      id: agentId,
      name: config.name || agentId,
      description: config.description || '',
      classRef: agentClass,
      custom: true,
      registeredAt: new Date().toISOString(),
      ...config
    };

    this.customAgents.set(agentId, customConfig);
    this.agentTypes.set(agentId, customConfig);

    return customConfig;
  }

  // Get agent type configuration
  getAgentType(agentTypeId) {
    return this.agentTypes.get(agentTypeId) || null;
  }

  // Get all registered agent types
  getAllAgentTypes() {
    const types = [];
    for (const [id, config] of this.agentTypes.entries()) {
      types.push({
        id,
        name: config.name,
        description: config.description,
        custom: config.custom || false,
        capabilities: config.capabilities || [],
        registeredAt: config.registeredAt
      });
    }
    return types;
  }

  // Check if agent type exists
  hasAgentType(agentTypeId) {
    return this.agentTypes.has(agentTypeId);
  }

  // Get custom agent instance
  getCustomAgent(agentId) {
    const config = this.customAgents.get(agentId);
    if (!config || !config.classRef) return null;

    return new config.classRef(config);
  }

  // Remove custom agent
  removeCustomAgent(agentId) {
    this.customAgents.delete(agentId);
    this.agentTypes.delete(agentId);
  }

  // Load agent from module path
  async loadAgentFromModule(agentId, modulePath, exportName = 'default') {
    try {
      const module = await import(modulePath);
      const AgentClass = module[exportName];

      if (!AgentClass) {
        throw new Error(`Export '${exportName}' not found in ${modulePath}`);
      }

      this.registerCustomAgent(agentId, AgentClass);
      return AgentClass;
    } catch (error) {
      throw new Error(`Failed to load agent from ${modulePath}: ${error.message}`);
    }
  }

  // Get agent capabilities
  getAgentCapabilities(agentTypeId) {
    const agentType = this.getAgentType(agentTypeId);
    return agentType?.capabilities || [];
  }

  // Check if agent can perform action
  canPerformAction(agentTypeId, action) {
    const capabilities = this.getAgentCapabilities(agentTypeId);
    return capabilities.includes(action);
  }

  // Get agents by capability
  getAgentsByCapability(capability) {
    const agents = [];
    for (const [id, config] of this.agentTypes.entries()) {
      if (config.capabilities?.includes(capability)) {
        agents.push(id);
      }
    }
    return agents;
  }

  // Get agent file extensions
  getAgentFileExtensions(agentTypeId) {
    const agentType = this.getAgentType(agentTypeId);
    return agentType?.fileExtensions || [];
  }

  // Get agent system prompt
  getSystemPrompt(agentTypeId) {
    const agentType = this.getAgentType(agentTypeId);
    return agentType?.systemPrompt || '';
  }

  // Export agent type configuration
  exportAgentType(agentTypeId) {
    const agentType = this.getAgentType(agentTypeId);
    if (!agentType) return null;

    const { classRef, ...config } = agentType;
    return config;
  }

  // Get registry statistics
  getRegistryStats() {
    return {
      totalAgentTypes: this.agentTypes.size,
      customAgents: this.customAgents.size,
      defaultAgents: this.agentTypes.size - this.customAgents.size,
      agents: Array.from(this.agentTypes.keys())
    };
  }
}

// Export singleton instance
export const agentRegistry = new AgentTypeRegistry();
