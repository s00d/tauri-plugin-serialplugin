import { defineCollection, defineContentConfig, z } from '@nuxt/content'

export default defineContentConfig({
  collections: {
    landing: defineCollection({
      type: 'page',
      source: 'index.md',
    }),
    docs: defineCollection({
      type: 'page',
      source: {
        include: '**',
        exclude: ['index.md'],
      },
      schema: z.object({
        // Set by scripts/generate.mjs on pages built from the sources (no "Edit this page" link).
        generated: z.boolean().optional(),
        links: z
          .array(
            z.object({
              label: z.string(),
              icon: z.string().optional(),
              to: z.string(),
              target: z.string().optional(),
            }),
          )
          .optional(),
      }),
    }),
  },
})
