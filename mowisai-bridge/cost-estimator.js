// Cost Estimation Module
// Tracks tokens and provides cost calculations

import { config } from './config.js';

export class CostEstimator {
  constructor() {
    this.estimations = [];
    this.tokenCounts = {
      input: 0,
      output: 0,
      total: 0
    };
  }

  // Rough token count estimation (GPT-style: ~1.3 tokens per word)
  estimateTokensFromText(text) {
    if (!text) return 0;
    const words = text.trim().split(/\s+/).length;
    return Math.ceil(words * 1.3);
  }

  // Estimate cost for a message
  estimateMessageCost(model, messageText) {
    const tokens = this.estimateTokensFromText(messageText);
    const rate = config.costPerToken[model];
    
    if (!rate) {
      return { tokens, cost: 0, error: `Unknown model: ${model}` };
    }

    const cost = tokens * rate.input; // Approximate as input for estimation
    return { tokens, cost, model };
  }

  // Estimate full conversation cost
  estimateConversationCost(model, messages) {
    let totalInputTokens = 0;
    let totalOutputTokens = 0;

    // Estimate input tokens from system messages and user messages
    messages.forEach((msg, idx) => {
      const tokens = this.estimateTokensFromText(msg.content);
      if (msg.role === 'user' || msg.role === 'system') {
        totalInputTokens += tokens;
      } else if (msg.role === 'assistant') {
        totalOutputTokens += tokens;
      }
    });

    const rate = config.costPerToken[model];
    if (!rate) {
      return { 
        inputTokens: totalInputTokens, 
        outputTokens: totalOutputTokens,
        cost: 0,
        error: `Unknown model: ${model}` 
      };
    }

    const inputCost = totalInputTokens * rate.input;
    const outputCost = totalOutputTokens * rate.output;
    const totalCost = inputCost + outputCost;

    const estimation = {
      model,
      inputTokens: totalInputTokens,
      outputTokens: totalOutputTokens,
      totalTokens: totalInputTokens + totalOutputTokens,
      inputCost: parseFloat(inputCost.toFixed(6)),
      outputCost: parseFloat(outputCost.toFixed(6)),
      totalCost: parseFloat(totalCost.toFixed(6)),
      timestamp: new Date()
    };

    this.estimations.push(estimation);
    this.tokenCounts.input += totalInputTokens;
    this.tokenCounts.output += totalOutputTokens;
    this.tokenCounts.total += (totalInputTokens + totalOutputTokens);

    return estimation;
  }

  // Track actual token usage
  recordActualUsage(model, inputTokens, outputTokens) {
    const rate = config.costPerToken[model];
    
    if (!rate) {
      return { error: `Unknown model: ${model}` };
    }

    const inputCost = inputTokens * rate.input;
    const outputCost = outputTokens * rate.output;
    const totalCost = inputCost + outputCost;

    const usage = {
      model,
      inputTokens,
      outputTokens,
      totalTokens: inputTokens + outputTokens,
      inputCost: parseFloat(inputCost.toFixed(6)),
      outputCost: parseFloat(outputCost.toFixed(6)),
      totalCost: parseFloat(totalCost.toFixed(6)),
      timestamp: new Date()
    };

    this.estimations.push(usage);
    this.tokenCounts.input += inputTokens;
    this.tokenCounts.output += outputTokens;
    this.tokenCounts.total += (inputTokens + outputTokens);

    return usage;
  }

  // Get cost summary
  getCostSummary() {
    let totalCost = 0;
    let totalTokens = 0;

    this.estimations.forEach(est => {
      totalCost += est.totalCost;
      totalTokens += est.totalTokens;
    });

    return {
      totalEstimations: this.estimations.length,
      totalTokens,
      totalCost: parseFloat(totalCost.toFixed(4)),
      tokenCounts: { ...this.tokenCounts },
      costByModel: this._aggregateByModel()
    };
  }

  _aggregateByModel() {
    const byModel = {};

    this.estimations.forEach(est => {
      if (!byModel[est.model]) {
        byModel[est.model] = {
          count: 0,
          tokens: 0,
          cost: 0
        };
      }
      byModel[est.model].count++;
      byModel[est.model].tokens += est.totalTokens;
      byModel[est.model].cost += est.totalCost;
    });

    // Format costs
    Object.keys(byModel).forEach(model => {
      byModel[model].cost = parseFloat(byModel[model].cost.toFixed(4));
    });

    return byModel;
  }

  // Get recent estimations
  getRecentEstimations(limit = 20) {
    return this.estimations.slice(-limit);
  }

  // Clear estimations
  reset() {
    this.estimations = [];
    this.tokenCounts = { input: 0, output: 0, total: 0 };
  }
}

// Export singleton instance
export const costEstimator = new CostEstimator();
