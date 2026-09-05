function (bam, gtf, stranded = 0, paired = FALSE, threads = 1, 
    verbose = FALSE, ...) 
{
    if (!file.exists(bam)) 
        stop("file", bam, "not found!")
    if (!file.exists(gtf)) 
        stop("file", gtf, "not found!")
    if (!is.logical(paired)) 
        stop("paired has to be either TRUE/FALSE")
    if (!is.numeric(stranded)) 
        stop("stranded has to be a number [0-2]")
    if (stranded < 0 || stranded > 2) 
        stop("stranded has to be a number [0-2]")
    if (!is.numeric(threads)) 
        stop("threads has to be a number")
    count <- function(mh, dup) {
        Rsubread::featureCounts(files = bam, annot.ext = gtf, 
            isGTFAnnotationFile = TRUE, nthreads = threads, isPairedEnd = paired, 
            strandSpecific = stranded, ignoreDup = dup, countMultiMappingReads = mh, 
            ...)
    }
    if (verbose) {
        counts <- list(mhdup = count(mh = TRUE, dup = FALSE), 
            mhnodup = count(mh = TRUE, dup = TRUE), nomhdup = count(mh = FALSE, 
                dup = FALSE), nomhnodup = count(mh = FALSE, dup = TRUE))
    }
    else {
        silencer <- capture.output(counts <- list(mhdup = count(mh = TRUE, 
            dup = FALSE), mhnodup = count(mh = TRUE, dup = TRUE), 
            nomhdup = count(mh = FALSE, dup = FALSE), nomhnodup = count(mh = FALSE, 
                dup = TRUE)))
    }
    x <- lapply(counts, function(x) {
        N <- sum(x$stat[, 2]) - x$stat[x$stat$Status == "Unassigned_Unmapped", 
            2]
        x <- data.frame(gene = rownames(x$counts), width = x$annotation$Length[match(rownames(x$counts), 
            x$annotation$GeneID)], counts = x$counts[, 1], RPK = 0, 
            RPKM = 0)
        x$RPK <- x$counts * (10^3/x$width)
        x$RPKM <- x$RPK * (10^6/N)
        return(x)
    })
    x <- data.frame(ID = x[[1]]$gene, geneLength = x[[1]]$width, 
        allCountsMulti = x[[1]]$counts, filteredCountsMulti = x[[2]]$counts, 
        dupRateMulti = (x[[1]]$counts - x[[2]]$counts)/x[[1]]$counts, 
        dupsPerIdMulti = x[[1]]$counts - x[[2]]$counts, RPKMulti = x[[1]]$RPK, 
        PKMMulti = x[[1]]$RPKM, allCounts = x[[3]]$counts, filteredCounts = x[[4]]$counts, 
        dupRate = (x[[3]]$counts - x[[4]]$counts)/x[[3]]$counts, 
        dupsPerId = x[[3]]$counts - x[[4]]$counts, RPK = x[[3]]$RPK, 
        RPKM = x[[3]]$RPKM)
}
<bytecode: 0xc768a3db0588>
<environment: namespace:dupRadar>
